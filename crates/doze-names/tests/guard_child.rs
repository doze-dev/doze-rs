//! The test guard reaches the programs a test starts, not only the test binary: a built `doze-names` run from the
//! target folder (as a test runs it, under cargo's environment) with no `DOZE_HOME` stops before it writes a guarded
//! doze home. The guarded home is a stand-in under the temp folder (`DOZE_GUARDED_HOME`), so this never goes near the
//! real one; the command, `untrust --ca --print`, makes the authority and then only prints.
#![cfg(feature = "setup")] // WHY: the doze-names binary needs it
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;

fn run(home: &Path, guarded: bool) -> std::process::Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_doze-names"));
    c.args(["untrust", "--ca", "--print", "guard.example"])
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("DOZE_HOME")
        .env_remove("DOZE_REAL_MACHINE")
        .env_remove("DOZE_UNDER_TEST");
    if guarded {
        c.env("DOZE_GUARDED_HOME", home.join(".doze"));
    } else {
        c.env_remove("DOZE_GUARDED_HOME");
    }
    c.output().unwrap()
}

#[test]
fn a_program_a_test_starts_stops_before_writing_a_guarded_home() {
    let home = std::env::temp_dir().join(format!("doze-guard-child-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let ca = home.join(".doze").join("ca");

    let out = run(&home, true);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{said}");
    assert!(
        said.contains("a test tried to make the certificate authority in the real doze home"),
        "the guard stopped it: {said}"
    );
    assert!(!ca.join("root.crt").exists(), "nothing was written");

    // The same run with that home not guarded writes it: what stopped the first was the guard.
    let out = run(&home, false);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(ca.join("root.crt").is_file());
    let _ = std::fs::remove_dir_all(&home);
}
