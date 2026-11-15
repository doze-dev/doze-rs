//! The guards no Go test holds (E2 audit 1, m11): the live segment is never pruned, a follower cannot lose a wake-up,
//! and a tail's cursor stops before a line still being written.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

fn texts(lines: &[Line]) -> Vec<String> {
    lines.iter().map(|l| l.text.clone()).collect()
}

/// A newer run on disk (a second writer, as a daemon started over a live one is) puts the live run under the run limit;
/// its segment being written still stays, so what it writes next is kept.
#[test]
fn the_live_segment_is_never_pruned() {
    let t = tempfile::tempdir().unwrap();
    let opts = Options {
        max_runs: 1,
        max_bytes: 200,
    };
    let a = Store::with_options(t.path(), opts);
    let first = a.begin("api").unwrap();
    first.line(STDOUT, "the first line, long enough to fill a segment");
    let b = Store::with_options(t.path(), opts);
    let newer = b.begin("api").unwrap();
    assert_eq!((first.id(), newer.id()), (1, 2));
    // Over a quarter of the cap: a new segment, then a prune that finds run 1 older than the one run kept.
    first.line(STDOUT, "written after the newer run began");
    let kept = a.tail("api", 1, 10).unwrap();
    assert_eq!(texts(&kept), ["written after the newer run began"]);
}

/// A line written after the follower's read and before it waits still wakes it: the waiter is taken before the read.
/// `emit` writes the line, so it lands exactly in that gap.
#[test]
fn a_line_written_while_emitting_wakes_the_follower() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::open(t.path());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let r = s.begin("api").unwrap();
        r.line(STDOUT, "first");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let follow = {
            let seen = seen.clone();
            s.follow("api", Cursor::default(), move |l| {
                if l.text == "first" {
                    r.line(STDOUT, "second");
                }
                let mut seen = lock(&seen);
                seen.push(l.text);
                seen.len() < 2
            })
        };
        let ended = tokio::time::timeout(Duration::from_secs(5), follow).await;
        assert!(matches!(ended, Ok(Ok(()))), "{ended:?}");
        assert_eq!(*lock(&seen), ["first", "second"]);
    });
}

/// A tail taken while a line is half written gives a cursor at that line's start, so following on from it reads the
/// line whole once its end is written.
#[test]
fn a_tail_cursor_stops_before_a_torn_line() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("api").join("r000001.s000.log");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "2026-10-10T09:15:01.000000Z o whole\n2026-10-10T09:15:02.000000Z o ha",
    )
    .unwrap();
    let s = Store::open(t.path());
    let (lines, cursor) = s.tail_at("api", 0, 10).unwrap();
    assert_eq!(texts(&lines), ["whole"]);
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(b"lf\n").unwrap();
    let (next, _) = s.read("api", cursor, 10).unwrap();
    assert_eq!(texts(&next), ["half"]);
}
