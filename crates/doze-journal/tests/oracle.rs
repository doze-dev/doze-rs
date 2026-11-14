//! The reader against journals the Go code wrote (tools/doze-oracle): one recorded in tests/fixtures, so this runs
//! anywhere, and one written now when Go and a doze checkout beside doze-rs are here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use doze_journal::{Event, parse_line, read};
use std::path::{Path, PathBuf};
use std::process::Command;

/// What Go's `journal.Read` returned, one event per line as Go marshals it.
fn go_read(text: &str) -> Vec<Event> {
    text.lines()
        .map(|l| parse_line(l.as_bytes()).expect(l))
        .collect()
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn reads_the_recorded_go_journal_as_go_does() {
    let ours = read(&fixtures().join("journal.jsonl")).unwrap();
    let go = go_read(&std::fs::read_to_string(fixtures().join("go-read.jsonl")).unwrap());
    assert_eq!(ours, go);
    // Rotation at 600 bytes kept seq 6 on; the torn seq 13 is skipped.
    let seqs: Vec<u64> = ours.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (6..=12).collect::<Vec<_>>());
    let watch = ours
        .iter()
        .find(|e| e.cause.kind == doze_journal::BY_WATCH)
        .unwrap();
    assert_eq!(watch.cause.by, "src/app.ts");
    assert_eq!(watch.time.unwrap().nanosecond(), 123_456_789);
}

#[test]
fn every_kind_reads_back() {
    let ours = read(&fixtures().join("journal.jsonl")).unwrap();
    for kind in [
        doze_journal::RESTART,
        doze_journal::GIVE_UP,
        doze_journal::PIN,
        doze_journal::SLEEP,
        doze_journal::FAILED,
        doze_journal::WAKE,
    ] {
        assert!(ours.iter().any(|e| e.kind == kind), "{kind}");
    }
}

fn oracle() -> Option<(PathBuf, PathBuf)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doze = root.join("../doze");
    let go = Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    (go && doze.join("internal/journal/journal.go").exists())
        .then(|| (root.join("tools/doze-oracle/run.sh"), doze))
}

fn run(script: &Path, doze: &Path, path: &Path) -> String {
    let out = Command::new("sh")
        .arg(script)
        .arg(doze)
        .arg("journal")
        .arg(path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn reads_a_journal_go_writes_now() {
    let Some((script, doze)) = oracle() else {
        eprintln!("skipped: needs Go and ../doze beside doze-rs");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    let go = go_read(&run(&script, &doze, &path));
    assert!(!go.is_empty());
    assert_eq!(read(&path).unwrap(), go);
}

/// Lines on the edges of Go's decoder (tests/fixtures/edges.txt, one per line), and what Go's `journal.Read` made of
/// each as a journal of its own (go-edges.jsonl: the event, or `skip`).
fn edges() -> Vec<Vec<u8>> {
    let raw = std::fs::read(fixtures().join("edges.txt")).unwrap();
    let body = raw.strip_suffix(b"\n").unwrap();
    body.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect()
}

fn agree(answers: &str) {
    let answers: Vec<&str> = answers.lines().collect();
    let edges = edges();
    assert_eq!(answers.len(), edges.len());
    for (line, go) in edges.iter().zip(answers) {
        let rust = doze_journal::parse(line).unwrap();
        let go: Vec<Event> = if go == "skip" {
            Vec::new()
        } else {
            vec![parse_line(go.as_bytes()).unwrap()]
        };
        assert_eq!(rust, go, "line {:?}", String::from_utf8_lossy(line));
    }
}

#[test]
fn go_and_rust_agree_on_every_recorded_edge_line() {
    agree(&std::fs::read_to_string(fixtures().join("go-edges.jsonl")).unwrap());
}

#[test]
fn go_and_rust_agree_on_every_edge_line_now() {
    let Some((script, doze)) = oracle() else {
        eprintln!("skipped: needs Go and ../doze beside doze-rs");
        return;
    };
    let out = Command::new("sh")
        .arg(&script)
        .arg(&doze)
        .arg("lines")
        .arg(fixtures().join("edges.txt"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    agree(&String::from_utf8(out.stdout).unwrap());
}
