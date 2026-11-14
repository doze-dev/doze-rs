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
fn with_no_daemon_up_says_starting_it_is_not_built() {
    let (sb, host, _) = setup::prepare(&["project shop".to_owned()]);
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert!(ran.stderr.contains("DZ0001"), "{}", ran.stderr);
    // The names are still checked first, as Go checks them.
    let typo = fixture::run(&sb, &host, Terminals::default(), false, &["up", "api2"]);
    assert_eq!(typo.code, 4, "{}", typo.stderr);
}
