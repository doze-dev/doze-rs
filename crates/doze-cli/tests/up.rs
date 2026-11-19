//! `doze up` against a running daemon (Go cmd/doze/up.go and remember.go): op `up` for each target and what it
//! needs, needs first; what was started remembered in the project's record, and a bare `up` choosing from it (`--all`,
//! `--last`, the question on a terminal, `-y`, a warning without one). Checked on the requests the daemon received
//! and on the record written.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/setup.rs"]
mod setup;

use doze_cli::Terminals;
use doze_control::fixture::FixtureDaemon;

const TTY: Terminals = Terminals {
    stdin: true,
    stdout: true,
    stderr: true,
};

fn setup() -> (fixture::Sandbox, fixture::FakeHost, FixtureDaemon) {
    let lines = vec!["project shop".to_owned(), "daemon up".to_owned()];
    let (sb, host, d) = setup::prepare(&lines);
    (sb, host, d.unwrap())
}

/// The `up` requests the daemon has had, as `up <name>`; each call drains what it reports.
fn ups(d: &FixtureDaemon, seen: &mut usize) -> Vec<String> {
    let all: Vec<String> = d
        .requests()
        .into_iter()
        .filter(|r| r.op == "up")
        .map(|r| format!("up {}", r.db))
        .collect();
    let new = all.get(*seen..).unwrap_or_default().to_vec();
    *seen = all.len();
    new
}

fn record(sb: &fixture::Sandbox) -> doze_projects::Project {
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    doze_projects::load(key.dir()).unwrap().expect("no record")
}

const EVERYTHING: [&str; 6] = [
    "up db",
    "up cache",
    "up events",
    "up api",
    "up worker",
    "up web",
];

#[test]
fn up_wakes_what_is_asked_and_what_it_needs_then_remembers_it() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, d) = setup();
    let mut seen = 0;
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["up", "api"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ups(&d, &mut seen), ["up db", "up api"]);
    assert_eq!(
        ran.stdout,
        "✓ db awake in 38ms\n✓ api awake in 1.2s\n› up — supervised in the background. doze for the dash · doze down to stop\n"
    );
    let r = record(&sb);
    assert_eq!(r.last_set, ["api"]);
    assert_eq!(r.name, "shop");
    assert_eq!(
        r.config,
        sb.project().join("doze.hcl").display().to_string()
    );
    assert!(r.last_up.is_some());

    // No terminal: everything, and how to get last time's set.
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ups(&d, &mut seen), EVERYTHING);
    assert!(
        ran.stderr
            .contains("last time only api started; doze up --last starts just those"),
        "{}",
        ran.stderr
    );
    assert!(
        record(&sb).last_set.is_empty(),
        "everything is recorded as empty"
    );
}

#[test]
fn a_bare_up_chooses_from_last_time() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, d) = setup();
    let mut seen = 0;
    let first = fixture::run(&sb, &host, Terminals::default(), false, &["up", "api"]);
    assert_eq!(first.code, 0, "{}", first.stderr);
    ups(&d, &mut seen);

    let last = fixture::run(&sb, &host, Terminals::default(), false, &["up", "--last"]);
    assert_eq!(last.code, 0, "{}", last.stderr);
    assert_eq!(ups(&d, &mut seen), ["up db", "up api"]);

    // A terminal: asked, and yes is the default.
    host.answers.borrow_mut().push_back(String::new());
    let asked = fixture::run(&sb, &host, TTY, false, &["up"]);
    assert_eq!(asked.code, 0, "{}", asked.stderr);
    assert!(
        asked.stderr.contains("Last time: api. Start the same?"),
        "{}",
        asked.stderr
    );
    assert_eq!(ups(&d, &mut seen), ["up db", "up api"]);

    // `a` is everything; then nothing smaller is recorded, so the next bare up does not ask.
    host.answers.borrow_mut().push_back("a".into());
    let all = fixture::run(&sb, &host, TTY, false, &["up"]);
    assert_eq!(all.code, 0, "{}", all.stderr);
    assert_eq!(ups(&d, &mut seen), EVERYTHING);
    let again = fixture::run(&sb, &host, TTY, false, &["up", "--last"]);
    assert!(
        again
            .stderr
            .contains("nothing smaller was started last time: starting everything"),
        "{}",
        again.stderr
    );
    assert_eq!(ups(&d, &mut seen), EVERYTHING);
}

#[test]
fn yes_on_a_terminal_takes_last_times_set_and_says_so() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, d) = setup();
    let mut seen = 0;
    fixture::run(&sb, &host, Terminals::default(), false, &["up", "web"]);
    ups(&d, &mut seen);
    let ran = fixture::run(&sb, &host, TTY, false, &["-y", "up"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(
        ran.stderr.contains("starting last time's set: web"),
        "{}",
        ran.stderr
    );
    assert_eq!(ups(&d, &mut seen), ["up web"]);
    // --all decides without asking.
    let all = fixture::run(&sb, &host, TTY, false, &["up", "--all"]);
    assert_eq!(all.code, 0, "{}", all.stderr);
    assert_eq!(ups(&d, &mut seen), EVERYTHING);
}

#[test]
fn up_refuses_before_it_asks_anything() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, d) = setup();
    let mut seen = 0;
    for (args, code) in [
        (&["up", "api", "--last"][..], 2),
        (&["up", "nope"][..], 4),
        (&["--json", "up", "-f"][..], 2),
        (&["up", "--last", "--all"][..], 2),
    ] {
        let ran = fixture::run(&sb, &host, Terminals::default(), false, args);
        assert_eq!(ran.code, code, "{args:?}: {}", ran.stderr);
    }
    assert!(ups(&d, &mut seen).is_empty());
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    assert!(
        doze_projects::load(key.dir()).unwrap().is_none(),
        "a refused up remembered something"
    );
}

#[test]
fn a_dry_run_asks_nothing_and_remembers_nothing() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, d) = setup();
    let mut seen = 0;
    let ran = fixture::run(&sb, &host, TTY, false, &["up", "api", "--dry-run"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stdout.contains("applies this"), "{}", ran.stdout);
    assert!(ups(&d, &mut seen).is_empty());
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    assert!(doze_projects::load(key.dir()).unwrap().is_none());
}

#[test]
fn a_task_is_done_not_awake() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, _d) = setup::prepare(&["project bare".to_owned(), "daemon tasks".to_owned()]);
    let _d = _d.unwrap();
    std::fs::write(
        sb.project().join("doze.hcl"),
        "task \"migrate\" { run = \"true\" }\n",
    )
    .unwrap();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["up", "migrate"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stdout.starts_with("✓ migrate done"), "{}", ran.stdout);
}

#[test]
fn with_no_daemon_up_starts_one_or_says_why_it_could_not() {
    let (sb, host, _) = setup::prepare(&["project shop".to_owned()]);
    // The names are checked first, as Go checks them.
    let typo = fixture::run(&sb, &host, Terminals::default(), false, &["up", "api2"]);
    assert_eq!(typo.code, 4, "{}", typo.stderr);
    // The plan says the daemon starts; nothing is started or remembered.
    let plan = fixture::run(
        &sb,
        &host,
        Terminals::default(),
        false,
        &["up", "api", "--dry-run"],
    );
    assert_eq!(plan.code, 0, "{}", plan.stderr);
    assert!(plan.stdout.contains("start the daemon"), "{}", plan.stdout);
    // A host that names no program to start: said, exit 1. Windows has no control socket for the daemon yet (core plan
    // W): that is said instead, exit 1 too.
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    let why = if cfg!(windows) {
        "no local control socket"
    } else {
        "could not find this doze program"
    };
    assert!(ran.stderr.contains(why), "{}", ran.stderr);
}

/// The real thing: `up` starts the built `doze` as the stack's daemon (D1's `__daemon`), wakes the app, and `down`
/// stops it, leaving no process, socket or pid file.
#[test]
fn up_starts_the_daemon_and_down_stops_it() {
    if !std::path::Path::new("/bin/sh").exists() || !setup::sockets() {
        return; // the app's run line goes through `sh -c`; Windows runs commands its own way (W)
    }
    let (sb, mut host, _) = setup::prepare(&["project bare".to_owned()]);
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let marker = sb.root.join("web.ran");
    // `home` keeps the daemon in the sandbox whatever this process's environment says (never ~/.doze).
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\n\napp \"web\" {{\n  run = \"echo up > {} && exec sleep 600\"\n}}\n",
        sb.home().display(),
        marker.display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let up = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    let pid = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let _reap = Reap(pid); // a failed assert below still stops the daemon
    let stop = |pid: Option<u32>| {
        if let Some(pid) = pid.filter(|p| doze_os::process::alive(*p)) {
            let _ = doze_os::process::terminate(pid);
        }
    };
    if up.code != 0 {
        stop(pid);
        panic!("{}\n{}", up.stdout, up.stderr);
    }
    assert!(up.stdout.contains("✓ web awake"), "{}", up.stdout);
    assert!(marker.exists(), "the app ran");
    assert!(record(&sb).last_up.is_some());
    let down = fixture::run(&sb, &host, Terminals::default(), false, &["--json", "down"]);
    if down.code != 0 {
        stop(pid);
    }
    assert_eq!(down.code, 0, "{}", down.stderr);
    assert!(down.stdout.contains("\"stopped\": true"), "{}", down.stdout);
    assert!(
        pid.is_some_and(|p| !doze_os::process::alive(p)),
        "the daemon is gone"
    );
    assert!(!key.socket().exists() && !key.pid_file().exists());
}

/// `doze run`: the daemon is started first, then the command runs (here the host records it), with `--env` too.
#[test]
fn run_starts_the_daemon_then_the_command() {
    if !std::path::Path::new("/bin/sh").exists() || !setup::sockets() {
        return;
    }
    let (sb, mut host, _) = setup::prepare(&["project bare".to_owned()]);
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\n\napp \"web\" {{\n  run = \"exec sleep 600\"\n}}\n",
        sb.home().display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let env = fixture::run(
        &sb,
        &host,
        Terminals::default(),
        false,
        &["run", "--env", "--", "true"],
    );
    // --env: the modules' variables (an app stack answers none), then the command.
    assert_eq!(env.code, 0, "{}", env.stderr);
    let ran = fixture::run(
        &sb,
        &host,
        Terminals::default(),
        false,
        &["run", "--", "mytool", "a", "b"],
    );
    let pid = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let _reap = Reap(pid); // a failed assert below still stops the daemon
    let down = fixture::run(&sb, &host, Terminals::default(), false, &["down"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(pid.is_some(), "the daemon was started");
    assert_eq!(down.code, 0, "{}", down.stderr);
    assert_eq!(
        host.spawned.borrow().last().cloned().unwrap_or_default(),
        ["run mytool", "a", "b"]
    );
}

/// Stops the daemon a test started (by its exact pid, while it lives) however the test ends.
struct Reap(Option<u32>);

impl Drop for Reap {
    fn drop(&mut self) {
        if let Some(pid) = self.0.filter(|p| doze_os::process::alive(*p)) {
            let _ = doze_os::process::terminate(pid);
        }
    }
}

/// A doze.hcl that moves the state folder (`data_dir`): `up` starts the daemon there and finds its socket there, the
/// record is written there, and `down` stops it there (audit 2, major 1, closed).
#[test]
fn a_moved_state_folder_is_followed() {
    if !std::path::Path::new("/bin/sh").exists() || !setup::sockets() {
        return;
    }
    let (sb, mut host, _) = setup::prepare(&["project bare".to_owned()]);
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let elsewhere = sb.root.join("elsewhere");
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\ndata_dir = \"{}\"\n\napp \"web\" {{\n  run = \"exec sleep 600\"\n}}\n",
        sb.home().display(),
        elsewhere.display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::new(&elsewhere);
    let up = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    let pid = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let _reap = Reap(pid);
    assert_eq!(up.code, 0, "{}\n{}", up.stdout, up.stderr);
    assert!(up.stdout.contains("✓ web awake"), "{}", up.stdout);
    assert!(
        pid.is_some(),
        "the daemon's pid file is in the moved folder"
    );
    assert!(
        doze_projects::load(&elsewhere)
            .unwrap()
            .is_some_and(|r| r.last_up.is_some())
    );
    let down = fixture::run(&sb, &host, Terminals::default(), false, &["--json", "down"]);
    assert_eq!(down.code, 0, "{}", down.stderr);
    assert!(down.stdout.contains("\"stopped\": true"), "{}", down.stdout);
    assert!(!key.socket().exists() && !key.pid_file().exists());
}

/// `sync` stops the daemon only when it started it itself: with doze stopped, the daemon it starts for the plan is gone
/// after; with a stack someone else runs (`up` here), the same sync leaves it running.
#[test]
fn sync_stops_only_the_daemon_it_started() {
    if !std::path::Path::new("/bin/sh").exists() || !setup::sockets() {
        return;
    }
    let (sb, mut host, _) = setup::prepare(&["project bare".to_owned()]);
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\n\napp \"web\" {{\n  run = \"exec sleep 600\"\n}}\n",
        sb.home().display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));

    let sync = fixture::run(&sb, &host, Terminals::default(), false, &["sync", "-y"]);
    assert_eq!(sync.code, 0, "{}", sync.stderr);
    assert!(
        sync.stderr
            .contains("started the doze daemon for doze sync"),
        "{}",
        sync.stderr
    );
    assert!(
        !key.pid_file().exists() && !key.socket().exists(),
        "the daemon sync started is stopped after it"
    );

    let up = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    let pid = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let _reap = Reap(pid);
    assert_eq!(up.code, 0, "{}", up.stderr);
    let sync = fixture::run(&sb, &host, Terminals::default(), false, &["sync", "-y"]);
    assert_eq!(sync.code, 0, "{}", sync.stderr);
    assert!(
        !sync.stderr.contains("started the doze daemon"),
        "{}",
        sync.stderr
    );
    assert!(
        pid.is_some_and(doze_os::process::alive),
        "a daemon sync did not start is left running"
    );
    let down = fixture::run(&sb, &host, Terminals::default(), false, &["down"]);
    assert_eq!(down.code, 0, "{}", down.stderr);
}
