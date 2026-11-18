//! The real `doze` binary, as a person and as a script meet it: on a pseudo-terminal (colour, the once-a-day update
//! line) and in a pipe (neither), and how long the read commands take. No network: the doze home holds a fresh valkey
//! index (signed with the fixture's throwaway key, which this debug binary is told to trust through
//! DOZE_TEST_REGISTRY_KEY) and the real valkey docs, so nothing has a reason to go out.
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

/// A sandbox whose home knows doze 9.9.9 is out (checked just now, so no check starts), with a valkey 0.2.1 index
/// signed with the fixture key, the real valkey docs, and a project pinning valkey 0.2.1.
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
    std::fs::copy(real.join("meta.yaml"), cache.join("meta.yaml")).unwrap();
    std::fs::write(
        cache.join("index.yaml"),
        fixture::index_text(&fixture::index_for("valkey", &[("0.2.1", 1, "")])),
    )
    .unwrap();
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

/// The fixture key's public half, base64, as DOZE_TEST_REGISTRY_KEY takes it.
fn test_public_key() -> String {
    use base64ct::Encoding as _;
    base64ct::Base64::encode_string(fixture::test_key().verifying_key().as_bytes())
}

fn doze(sb: &Sandbox, args: &[&str]) -> Command {
    let mut c = Command::new(DOZE);
    c.args(args)
        .current_dir(sb.project())
        .env("DOZE_HOME", sb.home())
        .env("DOZE_TEST_REGISTRY_KEY", test_public_key())
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
    // The sandbox registry is signed with the fixture key, which only a debug doze trusts (test_registry_key). Since
    // the 2026-11-18 key rotation nothing is signed with the production key that a test could check in, so a release
    // run times `version` alone. Put the registry commands back here once the Rust modules publish and a real signed
    // index can be a fixture again.
    let registry: &[&[&str]] = if cfg!(debug_assertions) {
        &[&["modules", "docs", "valkey"], &["modules"]]
    } else {
        &[]
    };
    for args in std::iter::once(&["version"][..]).chain(registry.iter().copied()) {
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

/// Only a debug binary reads DOZE_TEST_REGISTRY_KEY. Run as `cargo test --release`, the same command with the
/// variable set must refuse the fixture-signed index, as any shipped doze does.
#[test]
fn release_ignores_the_test_key() {
    let sb = sandbox("release-key");
    let out = doze(&sb, &["modules", "docs", "valkey"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    if cfg!(debug_assertions) {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    } else {
        assert!(
            !out.status.success(),
            "a release build trusted DOZE_TEST_REGISTRY_KEY"
        );
    }
}
