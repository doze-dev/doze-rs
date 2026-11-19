//! `doze bug`'s report: versions, the stack's shape and never a value, doze.lock's pins, the daemon's log tail.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;

use doze_cli::Terminals;

#[test]
fn the_report_has_the_shape_and_not_the_values() {
    let sb = fixture::Sandbox::new("bug");
    let hcl = "stack = \"shop\"\n\nmodule valkey \"cache\" {\n  version  = 9\n  port     = 6390\n  password = \"hunter2\"\n}\n\napp \"api\" {\n  run  = \"go run .\"\n  port = 8080\n  env  = { SECRET = \"s3cret-value\" }\n}\n";
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    std::fs::write(sb.project().join("doze.lock"), fixture::lock_text(false)).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    std::fs::create_dir_all(key.run_dir()).unwrap();
    let log: String = (1..=60).map(|i| format!("line {i}\n")).collect();
    std::fs::write(key.log_file(), log).unwrap();
    let host = fixture::FakeHost::default();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["--json", "bug"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    let path = v["data"]["report"].as_str().unwrap();
    let report = std::fs::read_to_string(path).unwrap();
    for want in [
        "doze dev",
        "- module cache valkey version 9 port 6390",
        "- app api port 8080",
        "doze/valkey 0.2.1",
        "line 60",
        "line 21",
    ] {
        assert!(
            report.contains(want),
            "the report lacks {want:?}:\n{report}"
        );
    }
    for never in ["hunter2", "s3cret-value", "go run", "line 20\n"] {
        assert!(
            !report.contains(never),
            "the report has {never:?}:\n{report}"
        );
    }
}

/// A doze.hcl that moves the state folder (`data_dir`): `bug` quotes the daemon's log from there, and `doctor` asks the
/// daemon on the socket there, as every other command finds the stack (not the default `<home>/projects/<slug>`).
#[test]
fn bug_and_doctor_follow_a_moved_state_folder() {
    let sb = fixture::Sandbox::new("bugmoved");
    let state = sb.root.join("state");
    let hcl = format!(
        "stack = \"shop\"\ndata_dir = \"{}\"\n\napp \"api\" {{\n  run = \"true\"\n}}\n",
        state.display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let moved = doze_projects::StackKey::new(state.clone());
    std::fs::create_dir_all(moved.run_dir()).unwrap();
    std::fs::write(moved.log_file(), "the moved log\n").unwrap();
    // The default place has a log too: it is not the stack's.
    let default = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    std::fs::create_dir_all(default.run_dir()).unwrap();
    std::fs::write(default.log_file(), "the default place's log\n").unwrap();
    let host = fixture::FakeHost::default();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["--json", "bug"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    let report = std::fs::read_to_string(v["data"]["report"].as_str().unwrap()).unwrap();
    assert!(report.contains("the moved log"), "{report}");
    assert!(!report.contains("the default place's log"), "{report}");

    if doze_os::local::LocalListener::bind(&sb.root.join("probe.sock")).is_err() {
        return; // no local control socket here (Windows, core plan W)
    }
    let script = doze_control::fixture::Script::parse("empty", "").unwrap();
    let _daemon = doze_control::fixture::FixtureDaemon::start(&moved.socket(), script).unwrap();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["--json", "doctor"]);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    let daemon = v["data"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "daemon")
        .cloned()
        .unwrap();
    assert_eq!(daemon["detail"], "running", "{daemon}");
}
