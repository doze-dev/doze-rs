//! The writer, cursors and followers: Go's `logstore_test.go`, each test named after the Go one it holds.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

fn texts(lines: &[Line]) -> String {
    let v: Vec<String> = lines
        .iter()
        .map(|l| format!("{} {}", l.stream, l.text))
        .collect();
    v.join(" | ")
}

fn opts(max_runs: usize, max_bytes: u64) -> Options {
    Options {
        max_runs,
        max_bytes,
    }
}

/// `TestLinesAreAssembledAndTagged`
#[test]
fn lines_are_assembled_and_tagged() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::open(t.path());
    let r = s.begin("api").unwrap();
    let (mut out, mut err) = (r.writer(STDOUT), r.writer(STDERR));
    write!(out, "listening on ").unwrap();
    writeln!(err, "warn: slow").unwrap();
    write!(out, ":8080\nready\r\n").unwrap();
    r.line(&format!("{STEP_PREFIX}setup"), "migrated 3");
    write!(out, "no newline yet").unwrap();
    let (lines, _) = s.read("api", Cursor::default(), 100).unwrap();
    assert_eq!(
        texts(&lines),
        "e warn: slow | o listening on :8080 | o ready | s:setup migrated 3"
    );
    assert_eq!((lines[0].instance.as_str(), lines[0].run), ("api", 1));
    assert!(stamp::is_stamp(&lines[0].stamp));
    // Closing records the line that never got its newline.
    r.close();
    let (all, _) = s.read("api", Cursor::default(), 100).unwrap();
    assert_eq!(all.last().unwrap().text, "no newline yet");
    // A closed run takes nothing more.
    r.line(STDOUT, "late");
    assert_eq!(s.read("api", Cursor::default(), 100).unwrap().0.len(), 5);
}

/// `TestRunsOutliveTheProcess`
#[test]
fn runs_outlive_the_process() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::open(t.path());
    for text in ["first run", "second run"] {
        let r = s.begin("api").unwrap();
        r.line(STDOUT, text);
        r.close();
    }
    // A new store over the same folder: a daemon restart.
    let s = Store::open(t.path());
    let runs = s.runs("api").unwrap();
    assert_eq!(
        runs.iter().map(|r| (r.id, r.live)).collect::<Vec<_>>(),
        [(1, false), (2, false)]
    );
    assert!(!runs[0].started.is_empty() && runs[0].bytes > 0);
    assert_eq!(texts(&s.tail("api", 0, 10).unwrap()), "o second run");
    assert_eq!(texts(&s.tail("api", -1, 10).unwrap()), "o first run");
    assert_eq!(texts(&s.tail("api", 1, 10).unwrap()), "o first run");
    assert!(s.tail("api", -2, 10).is_err());
    let third = s.begin("api").unwrap();
    assert_eq!(third.id(), 3);
    assert!(s.runs("api").unwrap()[2].live);
    assert!(s.tail("never-ran", 0, 10).unwrap().is_empty());
}

/// `TestCursorResumesAcrossRuns`
#[test]
fn a_cursor_resumes_across_runs() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::open(t.path());
    let r = s.begin("api").unwrap();
    r.line(STDOUT, "a");
    r.line(STDOUT, "b");
    let (lines, cur) = s.read("api", Cursor::default(), 1).unwrap();
    assert_eq!(texts(&lines), "o a");
    r.line(STDOUT, "c");
    r.close();
    let r2 = s.begin("api").unwrap();
    r2.line(STDOUT, "d");
    let (lines, cur) = s.read("api", cur, 100).unwrap();
    assert_eq!(texts(&lines), "o b | o c | o d");
    assert_eq!(lines[2].run, 2);
    assert!(s.read("api", cur, 100).unwrap().0.is_empty());
}

/// `TestOldRunsArePruned`
#[test]
fn old_runs_are_pruned() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::with_options(t.path(), opts(2, 0));
    for i in 1..=4 {
        let r = s.begin("api").unwrap();
        r.line(STDOUT, &format!("run {i}"));
        r.close();
    }
    let ids: Vec<u32> = s.runs("api").unwrap().iter().map(|r| r.id).collect();
    assert_eq!(ids, [3, 4]);
    // The default keeps five.
    let s = Store::open(t.path().join("d"));
    for _ in 0..7 {
        s.begin("api").unwrap().close();
    }
    assert_eq!(s.runs("api").unwrap().len(), DEFAULT_MAX_RUNS);
}

/// `TestALongRunStaysUnderTheCap`: one run that prints without end stays under the cap, and keeps its newest lines.
#[test]
fn a_long_run_stays_under_the_cap() {
    let t = tempfile::tempdir().unwrap();
    const CAP: u64 = 64 * 1024;
    let s = Store::with_options(t.path(), opts(0, CAP));
    let r = s.begin("api").unwrap();
    let pad = "x".repeat(100);
    for i in 0..5000 {
        r.line(STDOUT, &format!("{i:05} {pad}"));
    }
    let files: Vec<u64> = std::fs::read_dir(t.path().join("api"))
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .collect();
    let total: u64 = files.iter().sum();
    assert!(total <= CAP + CAP / 4, "{total} bytes for a {CAP} cap");
    assert!(files.len() >= 2, "a long run is split into segments");
    let tail = s.tail("api", 0, 1).unwrap();
    assert!(tail[0].text.starts_with("04999 "), "{}", texts(&tail));
    // The segment being written is never pruned, even when it alone is over the cap.
    let s2 = Store::with_options(t.path().join("one"), opts(0, 100));
    let one = s2.begin("x").unwrap();
    for i in 0..10 {
        one.line(STDOUT, &format!("{i:03} {}", "z".repeat(80)));
    }
    assert_eq!(s2.tail("x", 0, 1).unwrap()[0].text[..3], *"009");
    // A cursor into the deleted part resumes from what is left.
    let from = Cursor {
        run: 1,
        seg: 0,
        offset: 10,
    };
    let (lines, _) = s.read("api", from, 1).unwrap();
    assert!(lines.len() == 1 && !lines[0].text.starts_with("00000"));
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

async fn until(seen: &Mutex<Vec<String>>, want: &str) {
    for _ in 0..1000 {
        if lock(seen).join(" ") == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("followed {:?}, want {want:?}", lock(seen).join(" "));
}

/// `TestFollow`: a follower is woken by writes, sees every line in order, and carries on into the next run.
#[test]
fn a_follower_sees_every_line_into_the_next_run() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::open(t.path());
    rt().block_on(async {
        let r = s.begin("api").unwrap();
        r.line(STDOUT, "before");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let follow = {
            let (s, seen) = (s.clone(), seen.clone());
            tokio::spawn(async move {
                s.follow("api", Cursor::default(), |l| {
                    lock(&seen).push(format!("{}:{}", l.run, l.text));
                    true
                })
                .await
            })
        };
        until(&seen, "1:before").await;
        for i in 0..50 {
            r.line(STDOUT, &format!("l{i}"));
        }
        r.close();
        s.begin("api").unwrap().line(STDERR, "second");
        let mut want = vec!["1:before".to_string()];
        want.extend((0..50).map(|i| format!("1:l{i}")));
        want.push("2:second".into());
        until(&seen, &want.join(" ")).await;
        follow.abort();
    });
}

/// `TestFollowBeforeTheFirstRun`: a follower of an instance that has not started yet waits for it; `emit` returning
/// false ends it.
#[test]
fn a_follower_waits_for_the_first_run() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::open(t.path());
    rt().block_on(async {
        let got = Arc::new(Mutex::new(Vec::new()));
        let follow = {
            let (s, got) = (s.clone(), got.clone());
            tokio::spawn(async move {
                s.follow("api", Cursor::default(), |l| {
                    lock(&got).push(l.text);
                    false
                })
                .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        s.begin("api").unwrap().line(STDOUT, "hello");
        let ended = tokio::time::timeout(Duration::from_secs(2), follow).await;
        assert!(matches!(ended, Ok(Ok(Ok(())))), "{ended:?}");
        assert_eq!(*lock(&got), ["hello"]);
    });
}

/// `TestConcurrentWriters`: no line is torn.
#[test]
fn concurrent_writers_never_tear_a_line() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::open(t.path());
    let r = s.begin("api").unwrap();
    std::thread::scope(|sc| {
        for stream in [STDOUT, STDERR] {
            let mut w = r.writer(stream);
            sc.spawn(move || {
                for i in 0..500 {
                    writeln!(w, "line {i} of a writer").unwrap();
                }
            });
        }
    });
    let (lines, _) = s.read("api", Cursor::default(), 5000).unwrap();
    assert_eq!(lines.len(), 1000);
    assert!(lines.iter().all(|l| l.text.ends_with("of a writer")));
}

/// `TestTailReadsFromTheEnd`: the same lines whatever the size and `n`; the cursor is the end of the run, and stops
/// before a line still being written.
#[test]
fn tail_reads_from_the_end() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::with_options(t.path(), opts(0, 64 << 20));
    let r = s.begin("api").unwrap();
    const TOTAL: usize = 4000;
    let pad = "y".repeat(100);
    for i in 0..TOTAL {
        r.line(STDOUT, &format!("{i:05} {pad}"));
    }
    for n in [0, 1, 2, 199, 200, 1000, TOTAL, TOTAL + 50] {
        let (lines, end) = s.tail_at("api", 0, n).unwrap();
        let want = n.min(TOTAL);
        assert_eq!(lines.len(), want, "tail {n}");
        for (i, l) in lines.iter().enumerate() {
            assert_eq!(l.text[..5], format!("{:05}", TOTAL - want + i), "tail {n}");
        }
        assert!(s.read("api", end, 10).unwrap().0.is_empty(), "tail {n}");
    }
    r.writer(STDOUT).push(b"half a li");
    let (lines, end) = s.tail_at("api", 0, 1).unwrap();
    assert_eq!(lines[0].text[..5], format!("{:05}", TOTAL - 1));
    r.close();
    let (more, _) = s.read("api", end, 10).unwrap();
    assert_eq!(texts(&more), "o half a li");
}

/// Rust's own: a line longer than [`MAX_LINE`] without a newline is cut into lines of that size, so a process
/// that never prints one cannot grow doze's memory; `remove` deletes everything and closes the live run.
#[test]
fn an_endless_line_is_cut_and_remove_deletes_everything() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::open(t.path());
    let r = s.begin("api").unwrap();
    let w = r.writer(STDOUT);
    w.push(&vec![b'z'; MAX_LINE * 2 + 10]);
    w.push(b"\n");
    let (lines, _) = s.read("api", Cursor::default(), 10).unwrap();
    let lens: Vec<usize> = lines.iter().map(|l| l.text.len()).collect();
    assert_eq!(lens, [MAX_LINE, MAX_LINE, 10]);
    s.remove("api").unwrap();
    assert!(!t.path().join("api").exists() && r.is_closed());
    assert!(s.live("api").is_none());
}

/// E2 audit 1, m8: a chunk of many lines goes in one write, and still moves to a new segment at a quarter of the cap
/// mid-chunk: every line lands, in order, in segments each under the quarter.
#[test]
fn a_chunk_of_lines_is_split_across_segments_in_order() {
    let t = tempfile::tempdir().unwrap();
    const CAP: u64 = 16 * 1024;
    let s = Store::with_options(t.path(), opts(0, CAP));
    let r = s.begin("api").unwrap();
    let w = r.writer(STDOUT);
    let chunk: String = (0..100)
        .map(|i| format!("{i:03} {}\n", "y".repeat(60)))
        .collect();
    w.push(chunk.as_bytes());
    let segs: Vec<u64> = std::fs::read_dir(t.path().join("api"))
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .collect();
    assert!(segs.len() >= 2, "{segs:?}");
    assert!(segs.iter().all(|n| *n <= CAP / 4), "{segs:?}");
    let all = s.tail("api", 0, 1000).unwrap();
    let firsts: Vec<String> = all.iter().map(|l| l.text[..3].to_string()).collect();
    let want: Vec<String> = (0..100).map(|i| format!("{i:03}")).collect();
    // The oldest segments may be pruned to the cap; what is left is the newest lines, in order.
    assert_eq!(firsts, want[want.len() - firsts.len()..]);
    assert_eq!(firsts.last().map(String::as_str), Some("099"));
}
