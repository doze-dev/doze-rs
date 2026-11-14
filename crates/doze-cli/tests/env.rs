//! `doze env` with doze stopped starts the stack's daemon to ask the modules (as `doze run` starts it), says so, and
//! wakes nothing; a stack of apps answers no variables, which is said on stderr.
#![cfg(unix)]
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

#[test]
fn env_starts_the_daemon_to_ask_and_wakes_nothing() {
    if !setup::sockets() {
        return;
    }
    let (sb, mut host, _) = setup::prepare(&["project bare".to_owned()]);
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let marker = sb.root.join("web.ran");
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\n\napp \"web\" {{\n  run = \"echo up > {} && exec sleep 600\"\n}}\n",
        sb.home().display(),
        marker.display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let env = fixture::run(&sb, &host, Terminals::default(), false, &["env"]);
    let pid = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let down = fixture::run(&sb, &host, Terminals::default(), false, &["down"]);
    if let Some(pid) = pid.filter(|p| doze_os::process::alive(*p)) {
        let _ = doze_os::process::terminate(pid);
    }
    assert_eq!(env.code, 0, "{}\n{}", env.stdout, env.stderr);
    assert!(pid.is_some(), "the daemon was started");
    assert!(
        env.stderr.contains("started the doze daemon for doze env"),
        "{}",
        env.stderr
    );
    assert!(
        env.stderr
            .contains("no services with connection URLs declared"),
        "{}",
        env.stderr
    );
    assert!(!marker.exists(), "env woke the app");
    assert_eq!(down.code, 0, "{}", down.stderr);
}
