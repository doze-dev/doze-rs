//! `doze up`, `run` and `sync` with the real daemon: the built `doze` started as the stack's daemon (D1's
//! `__daemon`) and stopped again, its state folder followed, its signals passed on.
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

fn record(sb: &fixture::Sandbox) -> doze_projects::Project {
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    doze_projects::load(key.dir()).unwrap().expect("no record")
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

/// `doze run` as a person's IDE meets it: a SIGINT sent to doze alone (`kill -INT <doze>`, an IDE's stop) reaches the
/// command, once, and doze exits with the command's code. Before, doze took it and the command never heard it.
#[cfg(unix)]
#[test]
fn a_sigint_sent_to_doze_run_alone_reaches_the_command() {
    use std::os::unix::process::CommandExt;
    if !setup::sockets() {
        return;
    }
    let (sb, _, _) = setup::prepare(&["project bare".to_owned()]);
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\n\napp \"web\" {{\n  run = \"exec sleep 600\"\n}}\n",
        sb.home().display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let mark = sb.root.join("ready");
    let script = format!(
        "n=0; trap 'n=$((n+1))' INT; : > '{}'; i=0; while [ $i -lt 50 ]; do sleep 0.1; i=$((i+1)); done; exit $((40+n))",
        mark.display()
    );
    let mut doze = std::process::Command::new(env!("CARGO_BIN_EXE_doze"))
        .args(["run", "--", "/bin/sh", "-c", &script])
        .current_dir(sb.project())
        .env("DOZE_HOME", sb.home())
        .env("DOZE_ZONE", "off")
        .env("DOZE_NO_UPDATE_NOTIFIER", "1")
        .stdin(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    let t0 = std::time::Instant::now();
    while !mark.exists() && t0.elapsed() < std::time::Duration::from_secs(20) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let pid = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let _reap = Reap(pid);
    assert!(mark.exists(), "the command never started");
    std::thread::sleep(std::time::Duration::from_millis(100));
    let doze_pid = doze.id().to_string();
    let _ = std::process::Command::new("kill")
        .args(["-INT", &doze_pid])
        .status();
    let status = doze.wait().unwrap();
    let down = fixture::run(
        &sb,
        &fixture::FakeHost::default(),
        Terminals::default(),
        false,
        &["down"],
    );
    assert_eq!(status.code(), Some(41), "{status}");
    assert_eq!(down.code, 0, "{}", down.stderr);
}
