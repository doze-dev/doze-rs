//! The real `doze` binary, as a person and as a script meet it: on a pseudo-terminal (colour, the once-a-day update
//! line) and in a pipe (neither), and how long the read commands take. No network: the doze home holds fresh copies
//! of the real registry's files, so nothing has a reason to go out.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;

use fixture::Sandbox;
use std::io::Read as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DOZE: &str = env!("CARGO_BIN_EXE_doze");

/// A sandbox whose home knows doze 9.9.9 is out (checked just now, so no check starts), with this machine's copy of
/// the real valkey index and docs, and a project pinning valkey 0.2.1.
fn sandbox(tag: &str) -> Sandbox {
    let sb = Sandbox::new(tag);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let state = serde_json::json!({"checked_at": now, "latest": {"version": "9.9.9", "news": ["valkey sleeps with a snapshot"], "security": false}});
    std::fs::write(sb.home().join("update.json"), state.to_string()).unwrap();
    let real =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real/doze/valkey");
    let cache = sb.home().join("registry/doze/valkey");
    std::fs::create_dir_all(&cache).unwrap();
    for f in ["index.yaml", "meta.yaml"] {
        std::fs::copy(real.join(f), cache.join(f)).unwrap();
    }
    std::fs::write(
        sb.project().join("doze.hcl"),
        "valkey \"cache\" { version = 9 }\n",
    )
    .unwrap();
    std::fs::write(
        sb.project().join("doze.lock"),
        "modules:\n    doze/valkey:\n        version: 0.2.1\n        protocol: 1\n",
    )
    .unwrap();
    sb
}

fn doze(sb: &Sandbox, args: &[&str]) -> Command {
    let mut c = Command::new(DOZE);
    c.args(args)
        .current_dir(sb.project())
        .env("DOZE_HOME", sb.home())
        .env("TERM", "xterm-256color");
    for v in [
        "CI",
        "NO_COLOR",
        "DOZE_JSON",
        "DOZE_YES",
        "DOZE_NO_UPDATE_NOTIFIER",
        "DOZE_ZONE",
    ] {
        c.env_remove(v);
    }
    c
}

/// Runs doze on a new pseudo-terminal (stdin, stdout and stderr all on it) and returns everything it drew.
fn on_terminal(sb: &Sandbox, args: &[&str]) -> Option<(i32, String)> {
    let pty = match doze_os::pty::open() {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => return None,
        Err(e) => panic!("no pseudo-terminal: {e}"),
    };
    let mut cmd = doze(sb, args);
    cmd.stdin(pty.terminal.try_clone().unwrap())
        .stdout(pty.terminal.try_clone().unwrap())
        .stderr(pty.terminal.try_clone().unwrap());
    let mut child = cmd.spawn().unwrap();
    drop(cmd);
    drop(pty.terminal);
    let mut controller = pty.controller;
    let reader = std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 4096];
        // The terminal end closes with the child; reading then ends (EOF, or EIO on Linux).
        while let Ok(n) = controller.read(&mut buf) {
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
        }
        all
    });
    let code = child.wait().unwrap().code().unwrap_or(-1);
    let out = reader.join().unwrap();
    Some((code, String::from_utf8_lossy(&out).replace("\r\n", "\n")))
}

/// On a terminal: colour, and the update line once (the second run the same day says nothing).
#[test]
fn a_person_at_a_terminal_sees_colour_and_the_update_line_once() {
    let sb = sandbox("pty");
    let Some((code, first)) = on_terminal(&sb, &["up"]) else {
        eprintln!("skipped: no pseudo-terminal on this platform");
        return;
    };
    assert_eq!(code, 1, "{first}");
    assert!(
        first.contains("\x1b[1;31merror[DZ0001]:"),
        "no colour on a terminal:\n{first}"
    );
    assert!(
        first.contains("doze 9.9.9 is out (you have dev): valkey sleeps with a snapshot"),
        "no update line:\n{first}"
    );
    let (_, second) = on_terminal(&sb, &["up"]).unwrap();
    assert!(
        !second.contains("9.9.9"),
        "the update line twice in a day:\n{second}"
    );
}

/// In a pipe: the same words, no colour, no update line; the result on stdout and the problem on stderr.
#[test]
fn a_script_gets_plain_text_and_no_update_line() {
    let sb = sandbox("pipe");
    let out = doze(&sb, &["up"]).stdin(Stdio::null()).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty() && !err.contains('\x1b') && !err.contains("9.9.9"),
        "{err}"
    );
    let out = doze(&sb, &["version"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("doze dev ("));
    assert!(out.stderr.is_empty());
}

/// Rule 5's budget for read commands, on what they do warm: the proposal sets 15 ms for status, env and url (Go
/// takes 27 to 54 ms); those need the core, so the read commands that exist carry it now. A debug build gets more
/// room; the numbers are printed either way.
#[test]
fn read_commands_answer_within_the_budget() {
    let sb = sandbox("budget");
    let budget = if cfg!(debug_assertions) {
        Duration::from_millis(60)
    } else {
        Duration::from_millis(15)
    };
    for args in [
        &["version"][..],
        &["modules", "docs", "valkey"],
        &["modules"],
    ] {
        let mut times: Vec<Duration> = (0..15)
            .map(|_| {
                let t = Instant::now();
                let out = doze(&sb, args).stdin(Stdio::null()).output().unwrap();
                let took = t.elapsed();
                assert!(
                    out.status.success(),
                    "{args:?}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
                took
            })
            .collect();
        times.sort();
        let median = times[times.len() / 2];
        eprintln!(
            "doze {}: median {median:?} (budget {budget:?})",
            args.join(" ")
        );
        assert!(
            median < budget,
            "doze {} took {median:?}, over {budget:?}",
            args.join(" ")
        );
    }
}
