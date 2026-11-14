//! The writer: Go's `journal_test.go`, each test named after the Go one it holds, and what Go reads back of what Rust
//! writes (the recorded Go lines re-encoded byte for byte).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use doze_journal::{Cause, Event, Journal, encode, parse_line, rotated};
use std::io::Write;
use std::path::PathBuf;

fn ev(kind: &str, instance: &str) -> Event {
    Event {
        kind: kind.into(),
        instance: instance.into(),
        ..Event::default()
    }
}

fn path() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("run/journal.jsonl");
    (t, p)
}

/// `TestAppendAndReadBack`
#[test]
fn append_and_read_back() {
    let (_t, p) = path();
    let j = Journal::open(&p, 0).unwrap();
    let mut wake = ev("wake", "db");
    wake.cause = Cause {
        kind: "dependency".into(),
        by: "api".into(),
    };
    let wake = j.append(wake);
    let mut ready = ev("ready", "db");
    ready.took_ns = 38_000_000;
    let ready = j.append(ready);
    assert_eq!((wake.seq, ready.seq), (1, 2));
    assert!(wake.time.is_some());
    let all = j.since(0).unwrap();
    assert_eq!(all, [wake, ready.clone()]);
    assert_eq!(j.since(1).unwrap(), [ready]);
}

/// `TestSequenceSurvivesReopen`
#[test]
fn the_sequence_survives_a_reopen() {
    let (_t, p) = path();
    let j = Journal::open(&p, 0).unwrap();
    j.append(ev("wake", "db"));
    j.append(ev("ready", "db"));
    j.close();
    assert_eq!(
        Journal::open(&p, 0).unwrap().append(ev("sleep", "db")).seq,
        3
    );
}

/// `TestRotationKeepsOneOldFileAndTheSequence`
#[test]
fn rotation_keeps_one_old_file_and_the_sequence() {
    let (_t, p) = path();
    let j = Journal::open(&p, 400).unwrap();
    for _ in 0..40 {
        let mut e = ev("wake", "db");
        e.detail = "x".repeat(40);
        j.append(e);
    }
    for f in [&p, &rotated(&p)] {
        let len = std::fs::metadata(f).unwrap().len();
        assert!(len <= 400, "{} is {len} bytes", f.display());
    }
    let all = j.since(0).unwrap();
    assert_eq!(all.last().unwrap().seq, 40);
    assert!(all.windows(2).all(|w| w[1].seq == w[0].seq + 1));
    j.close();
    assert_eq!(
        Journal::open(&p, 400).unwrap().append(ev("sleep", "")).seq,
        41
    );
}

/// `TestSubscribeAndTail`
#[test]
fn subscribe_and_tail() {
    let (_t, p) = path();
    let j = Journal::open(&p, 0).unwrap();
    j.append(ev("wake", "old"));
    let mut rx = j.subscribe();
    j.append(ev("wake", "db"));
    let got = rx.try_recv().unwrap();
    assert_eq!((got.instance.as_str(), got.seq), ("db", 2));
    drop(rx);
    j.append(ev("sleep", "db"));
    let tail = j.tail(2).unwrap();
    assert_eq!(tail.len(), 2);
    assert_eq!(tail[1].kind, "sleep");
}

/// `TestTornLastLineIsSkipped`, and better than Go: the next event is a line of its own, so it reads back.
#[test]
fn a_torn_last_line_is_skipped_and_the_next_event_reads_back() {
    let (_t, p) = path();
    let j = Journal::open(&p, 0).unwrap();
    j.append(ev("wake", "db"));
    j.close();
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(br#"{"seq":2,"kind":"rea"#).unwrap();
    drop(f);
    let j = Journal::open(&p, 0).unwrap();
    assert_eq!(j.append(ev("ready", "db")).seq, 2);
    let kinds: Vec<String> = j.since(0).unwrap().into_iter().map(|e| e.kind).collect();
    assert_eq!(kinds, ["wake", "ready"]);
}

/// A slow subscriber is told how many it missed, and fills the gap from the file.
#[test]
fn a_lagging_subscriber_is_told_and_fills_the_gap() {
    let (_t, p) = path();
    let j = Journal::open(&p, 0).unwrap();
    let mut rx = j.subscribe();
    for _ in 0..doze_journal::SUBSCRIBER_BUFFER + 5 {
        j.append(ev("wake", "db"));
    }
    let missed = match rx.try_recv() {
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => n,
        other => panic!("{other:?}"),
    };
    assert_eq!(missed, 5);
    let next = rx.try_recv().unwrap();
    assert_eq!(next.seq, 6);
    assert_eq!(
        j.since(0).unwrap().len(),
        doze_journal::SUBSCRIBER_BUFFER + 5
    );
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// Every line Go marshalled (the recorded journal, and the edge cases: offsets, fractions, escapes, the zero time,
/// a negative `took`) is written back byte for byte, so the Go doze, the app and a Rust reader read what Rust writes.
#[test]
fn writes_what_go_writes_byte_for_byte() {
    let mut n = 0;
    for name in ["go-read.jsonl", "go-edges.jsonl"] {
        for line in fixture(name).lines().filter(|l| *l != "skip") {
            let e = parse_line(line.as_bytes()).expect(line);
            // Go's zero time at another offset reads as no time (the reader's choice); it is written in UTC.
            if e.time.is_none() && !line.contains("\"0001-01-01T00:00:00Z\"") {
                continue;
            }
            assert_eq!(encode(&e), line);
            n += 1;
        }
    }
    assert!(n >= 20, "{n} lines");
    // Go's HTML-safe escapes and its short ones.
    let mut e = ev("x", "<a&b>");
    e.seq = 1;
    e.detail = "\u{8}\u{c}\u{1}\u{2028}\"\\".into();
    assert_eq!(
        encode(&e),
        r#"{"seq":1,"time":"0001-01-01T00:00:00Z","kind":"x","instance":"\u003ca\u0026b\u003e","detail":"\b\f\u0001\u2028\"\\"}"#
    );
}
