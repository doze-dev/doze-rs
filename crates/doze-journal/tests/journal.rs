//! The reader against journals as the Go daemon leaves them: Go's own tests (internal/journal/journal_test.go), the
//! reading half of each, and the corners of Go's decoder a line can hit.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use doze_journal::{Event, MAX_LINE, parse, parse_line, read, rotated, since, tail};
use std::path::Path;
use std::time::Duration;

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

fn line(seq: u64, kind: &str, instance: &str) -> String {
    format!(
        r#"{{"seq":{seq},"time":"2026-11-13T09:30:00Z","kind":"{kind}","instance":"{instance}"}}"#
    )
}

// Go: TestAppendAndReadBack.
#[test]
fn reads_back_what_was_appended() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    write(
        &path,
        concat!(
            r#"{"seq":1,"time":"2026-11-13T09:30:00.5Z","kind":"wake","instance":"db","cause":{"kind":"dependency","by":"api"}}"#,
            "\n",
            r#"{"seq":2,"time":"2026-11-13T09:30:01Z","kind":"ready","instance":"db","took":38000000}"#,
            "\n"
        ),
    );
    let all = since(&path, 0).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].cause.kind, doze_journal::BY_DEPENDENCY);
    assert_eq!(all[0].cause.by, "api");
    assert!(all[0].time.is_some());
    assert_eq!(all[1].took(), Some(Duration::from_millis(38)));
    let after = since(&path, 1).unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].kind, doze_journal::READY);
}

// Go: TestRotationKeepsOneOldFileAndTheSequence (the reading half): rotated first, then live.
#[test]
fn reads_the_rotated_file_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    write(
        &rotated(&path),
        &format!("{}\n{}\n", line(3, "wake", "a"), line(4, "ready", "a")),
    );
    write(&path, &format!("{}\n", line(5, "sleep", "a")));
    let seqs: Vec<u64> = read(&path).unwrap().iter().map(|e| e.seq).collect();
    assert_eq!(seqs, [3, 4, 5]);
    assert_eq!(rotated(&path), dir.path().join("journal.jsonl.1"));
}

// Go: TestSubscribeAndTail (the Tail half).
#[test]
fn tail_is_the_last_n_oldest_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    write(
        &path,
        &format!(
            "{}\n{}\n{}\n",
            line(1, "wake", "old"),
            line(2, "wake", "db"),
            line(3, "sleep", "db")
        ),
    );
    let t = tail(&path, 2).unwrap();
    assert_eq!(t.iter().map(|e| e.seq).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(t[1].kind, doze_journal::SLEEP);
    assert_eq!(tail(&path, 10).unwrap().len(), 3);
    assert!(tail(&path, 0).unwrap().is_empty());
}

// Go: TestTornLastLineIsSkipped. The daemon appends after a torn line without a newline between, so the torn line and
// the next event are one line, which does not decode: both are lost, as in Go.
#[test]
fn a_torn_last_line_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    let torn = format!("{}\n{{\"seq\":2,\"kind\":\"rea", line(1, "wake", "db"));
    write(&path, &torn);
    assert_eq!(read(&path).unwrap().len(), 1);
    write(
        &path,
        &format!(
            "{torn}{}\n{}\n",
            line(2, "ready", "db"),
            line(3, "sleep", "db")
        ),
    );
    let seqs: Vec<u64> = read(&path).unwrap().iter().map(|e| e.seq).collect();
    assert_eq!(seqs, [1, 3]);
}

// Go: TestNilJournalIsANoOp. Here: a journal never written reads as empty.
#[test]
fn a_missing_journal_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read(&dir.path().join("none.jsonl")).unwrap().is_empty());
}

#[test]
fn a_directory_in_place_of_the_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("journal.jsonl")).unwrap();
    assert!(read(&dir.path().join("journal.jsonl")).is_err());
}

#[test]
fn lines_split_as_gos_scanner_splits_them() {
    let text = format!("{}\r\n\n{}", line(1, "wake", "a"), line(2, "wake", "b"));
    let seqs: Vec<u64> = parse(text.as_bytes())
        .unwrap()
        .iter()
        .map(|e| e.seq)
        .collect();
    assert_eq!(
        seqs,
        [1, 2],
        "CR dropped, empty line skipped, last line kept without its newline"
    );
    assert!(parse(b"").unwrap().is_empty());
    assert!(parse(b"\n").unwrap().is_empty());
}

#[test]
fn a_line_too_long_for_gos_scanner_fails_the_read() {
    let mut long = format!(r#"{{"seq":1,"detail":"{}"}}"#, "x".repeat(MAX_LINE));
    assert!(parse(long.as_bytes()).is_err());
    long = format!(r#"{{"seq":1,"detail":"{}"}}"#, "x".repeat(MAX_LINE - 100));
    assert_eq!(parse(long.as_bytes()).unwrap().len(), 1);
}

fn one(text: &str) -> Option<Event> {
    parse_line(text.as_bytes())
}

#[test]
fn keys_match_as_go_matches_them() {
    let e = one(r#"{"SEQ":4,"Kind":"wake","INSTANCE":"db","Cause":{"KIND":"idle"},"extra":[1,{"x":1e400}]}"#).unwrap();
    assert_eq!(
        (e.seq, e.kind.as_str(), e.instance.as_str()),
        (4, "wake", "db")
    );
    assert_eq!(e.cause.kind, "idle");
    // A repeated key: the last wins; a repeated object merges.
    let e = one(r#"{"seq":1,"seq":2,"cause":{"kind":"watch"},"cause":{"by":"a.ts"}}"#).unwrap();
    assert_eq!(e.seq, 2);
    assert_eq!(
        (e.cause.kind.as_str(), e.cause.by.as_str()),
        ("watch", "a.ts")
    );
    // null leaves a field as it was.
    let e = one(r#"{"seq":3,"kind":"wake","kind":null,"time":null,"cause":null}"#).unwrap();
    assert_eq!(e.kind, "wake");
    assert_eq!(e.time, None);
}

#[test]
fn lines_go_rejects_are_skipped() {
    for bad in [
        "",
        "null",
        "[]",
        r#"{"kind":"wake"}"#,
        r#"{"seq":0}"#,
        r#"{"seq":-1}"#,
        r#"{"seq":1.5}"#,
        r#"{"seq":"1"}"#,
        r#"{"seq":1,"took":"38ms"}"#,
        r#"{"seq":1,"kind":5}"#,
        r#"{"seq":1,"time":"yesterday"}"#,
        r#"{"seq":1,"cause":"idle"}"#,
        r#"{"seq":1} trailing"#,
        r#"{"seq":1,}"#,
    ] {
        assert_eq!(one(bad), None, "{bad}");
    }
}

#[test]
fn invalid_utf8_in_a_string_becomes_the_replacement_character() {
    let mut raw = br#"{"seq":1,"detail":"a"#.to_vec();
    raw.extend_from_slice(&[0xff, b'"', b'}']);
    assert_eq!(parse_line(&raw).unwrap().detail, "a\u{fffd}");
}

#[test]
fn a_time_keeps_its_offset_and_nanoseconds() {
    let e = one(r#"{"seq":1,"time":"2026-11-13T15:00:00.123456789+05:30"}"#).unwrap();
    let t = e.time.unwrap();
    assert_eq!(t.nanosecond(), 123_456_789);
    assert_eq!(t.offset().whole_minutes(), 330);
    assert_eq!(e.took(), None);
    assert_eq!(one(r#"{"seq":1,"took":-5}"#).unwrap().took(), None);
}
