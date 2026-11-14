//! The app's own administrator prompt, for real, on macOS: the system's password dialog, a script run as root, and
//! what it prints streamed back line by line. Ignored by default (it needs a person at the keyboard); the owner runs it
//! through `scripts/app-privileged-e2e-darwin.sh`, which sets `DOZE_ADMIN_E2E=1`.
//!
//! Nothing it runs as root changes the machine: it prints who it runs as and counts, with a pause between lines.

#![cfg(target_os = "macos")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use doze_desk::settings::{Cli, Fail, Hands, Said, SystemHands};

/// What the hands said, and when.
type Heard = Arc<Mutex<Vec<(Duration, Said)>>>;

fn hands() -> (SystemHands, Heard) {
    let said = Arc::new(Mutex::new(Vec::new()));
    let mut h = SystemHands::new(Cli::go("doze".into()), std::env::temp_dir());
    let (s, t0) = (said.clone(), Instant::now());
    h.out = Some(Arc::new(move |l: Said| {
        println!("{:>6} ms  {l:?}", t0.elapsed().as_millis());
        s.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((t0.elapsed(), l));
    }));
    (h, said)
}

fn wanted() -> bool {
    std::env::var("DOZE_ADMIN_E2E").is_ok_and(|v| v == "1")
}

#[test]
#[ignore = "raises the system's password prompt: scripts/app-privileged-e2e-darwin.sh"]
fn the_prompt_runs_a_script_as_root_and_streams_it() {
    if !wanted() {
        return;
    }
    println!("\n>>> type your password in the system's prompt (it runs: id -u, then counts to 3)");
    let (mut h, said) = hands();
    let script = "echo \"uid $(id -u)\"; for i in 1 2 3; do echo \"line $i\"; sleep 1; done";
    assert_eq!(h.admin("check the app's prompt (doze e2e)", script), Ok(()));
    let said = said
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let lines: Vec<(Duration, String)> = said
        .iter()
        .filter_map(|(t, l)| match l {
            Said::Line(s) => Some((*t, s.clone())),
            _ => None,
        })
        .collect();
    let words: Vec<&str> = lines.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(words, ["uid 0", "line 1", "line 2", "line 3"]);
    // Streamed: the lines came as they were printed, not all at the end.
    let first = lines.first().map(|l| l.0).unwrap_or_default();
    let last = lines.last().map(|l| l.0).unwrap_or_default();
    assert!(
        last - first >= Duration::from_millis(1500),
        "the lines came together ({first:?} .. {last:?}): not streamed"
    );
}

#[test]
#[ignore = "raises the system's password prompt: scripts/app-privileged-e2e-darwin.sh"]
fn a_closed_prompt_is_a_cancel() {
    if !wanted() {
        return;
    }
    println!("\n>>> press Cancel in the system's prompt this time");
    let (mut h, _) = hands();
    assert_eq!(
        h.admin(
            "check that Cancel is heard (doze e2e: press Cancel)",
            "echo should-not-run"
        ),
        Err(Fail::Cancelled)
    );
}
