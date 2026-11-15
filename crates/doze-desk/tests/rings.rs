//! The log ring and the meters: the Go hub's ring tests, the limits, and what 100,000 lines cost.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss)]

mod common;

use common::{epoch, script};
use doze_control::{LogFrame, StatView, Timestamp};
use doze_desk::{LINES_KEPT, LogRing, SAMPLES_KEPT, Series, TEXT_KEPT};
use std::time::{Duration, SystemTime};

fn at(t: SystemTime) -> Timestamp {
    Timestamp::from_system_time(t)
}

fn line(instance: &str, text: &str, secs: u64, run: i64) -> LogFrame {
    LogFrame {
        instance: instance.into(),
        line: text.into(),
        stream: "o".into(),
        run,
        time: at(epoch() + Duration::from_secs(secs)),
    }
}

#[test]
fn a_line_already_held_is_not_added_again() {
    let mut r = LogRing::new();
    for i in 0..5 {
        assert!(r.push(&line("a", "x", i, 1)));
    }
    assert!(
        !r.push(&line("a", "x", 4, 1)),
        "a reconnect's backlog repeats a line"
    );
    assert!(!r.push(&line("a", "x", 2, 1)), "an older one too");
    assert!(
        r.push(&line("a", "x", 4, 2)),
        "another run's line is another line"
    );
    assert!(r.push(&line("b", "x", 4, 1)), "so is another instance's");
    assert!(r.push(&line("a", "y", 4, 1)));
    assert_eq!(r.len(), 8);
    assert_eq!((r.held("a"), r.held("b"), r.held("")), (7, 1, 8));
    assert_eq!((r.added("a"), r.added("")), (7, 8));
    let last = r.get(7).unwrap();
    assert_eq!(
        (last.instance, last.line, last.stream, last.run),
        ("a", "y", "o", 1)
    );
    assert_eq!(last.time, Some(epoch() + Duration::from_secs(4)));
    assert_eq!(
        r.tail("a", 2)
            .iter()
            .map(|l| l.line.as_str())
            .collect::<Vec<_>>(),
        ["x", "y"]
    );
    assert_eq!(
        r.tail("a", 1).first().unwrap(),
        &line("a", "y", 4, 1),
        "a line reads back as it was sent"
    );
}

#[test]
fn the_ring_holds_the_last_hundred_thousand_lines() {
    let mut r = LogRing::new();
    let extra = 1_234;
    for i in 0..(LINES_KEPT + extra) {
        let inst = if i % 10 == 0 { "db" } else { "api" };
        r.push(&line(inst, &format!("line {i}"), i as u64, 1));
    }
    assert_eq!(r.len(), LINES_KEPT);
    assert_eq!(r.get(0).unwrap().line, format!("line {extra}"));
    assert_eq!(
        r.iter().last().unwrap().line,
        format!("line {}", LINES_KEPT + extra - 1)
    );
    assert_eq!(r.held("db") + r.held("api"), LINES_KEPT);
    assert_eq!(r.held("db"), r.of("db").count());
    assert_eq!(
        r.added(""),
        (LINES_KEPT + extra) as u64,
        "the version counts what slid out"
    );
}

#[test]
fn the_ring_holds_at_most_its_text_budget() {
    let mut r = LogRing::new();
    let long = "x".repeat(1000);
    for i in 0..30_000u64 {
        r.push(&line("api", &format!("{i:06} {long}"), i, 1));
    }
    let text: usize = r.iter().map(|l| l.line.len()).sum();
    assert!(text <= TEXT_KEPT, "{text} bytes held");
    assert!(
        text > TEXT_KEPT - 2_000,
        "{text} bytes held: the budget is used"
    );
    assert!(
        r.heap_bytes() < TEXT_KEPT + 3 * 1024 * 1024,
        "{} bytes",
        r.heap_bytes()
    );
    // A line longer than an arena is held whole.
    let huge = "y".repeat(200 * 1024);
    r.push(&line("api", &huge, 99_999, 1));
    assert_eq!(r.iter().last().unwrap().line.len(), huge.len());
}

/// Lines like the ones the fixture stacks print, numbered so each is new: what "typical" means here.
fn typical() -> Vec<LogFrame> {
    let mut lines: Vec<LogFrame> = ["shop", "larder", "crash", "mess"]
        .iter()
        .flat_map(|h| script(h).logs)
        .collect();
    lines.retain(|l| !l.line.is_empty());
    lines
}

#[test]
fn what_a_hundred_thousand_lines_cost() {
    let samples = typical();
    let mean = samples.iter().map(|l| l.line.len()).sum::<usize>() as f64 / samples.len() as f64;
    let mut r = LogRing::new();
    let mut naive = 0usize;
    for i in 0..LINES_KEPT {
        let s = &samples[i % samples.len()];
        let f = LogFrame {
            line: format!("{} #{i}", s.line),
            time: at(epoch() + Duration::from_millis(i as u64)),
            ..s.clone()
        };
        naive += std::mem::size_of::<LogFrame>()
            + f.line.len()
            + f.instance.len()
            + f.stream.len()
            + f.time.as_str().len();
        assert!(r.push(&f));
    }
    let bytes = r.heap_bytes();
    let text: usize = r.iter().map(|l| l.line.len()).sum();
    eprintln!(
        "log ring: {} lines, mean line {:.0} bytes ({mean:.0} before numbering): {} bytes held = {:.1} bytes a line, \
         {:.2} MB for 100k ({:.2} MB of it text); as Go-style frames it would be at least {:.2} MB",
        r.len(),
        text as f64 / r.len() as f64,
        bytes,
        bytes as f64 / r.len() as f64,
        bytes as f64 / 1e6,
        text as f64 / 1e6,
        naive as f64 / 1e6,
    );
    assert!(
        bytes <= 25_000_000,
        "100k typical lines within the 25 MB budget"
    );
    // Per line beyond its text: the 32-byte record and the arenas' slack.
    let overhead = (bytes - text) as f64 / r.len() as f64;
    assert!(
        overhead < 40.0,
        "{overhead:.1} bytes a line beyond the text"
    );
}

#[test]
fn a_ring_that_slides_for_ever_frees_its_arenas() {
    let mut r = LogRing::new();
    for i in 0..(3 * LINES_KEPT) {
        r.push(&line(
            "api",
            &format!("a line of a typical length, numbered {i:08}"),
            i as u64,
            1,
        ));
    }
    let text: usize = r.iter().map(|l| l.line.len()).sum();
    assert!(r.heap_bytes() < text + LINES_KEPT * 32 + 3 * 64 * 1024 + 1024 * 1024);
}

#[test]
fn a_reading_already_held_is_skipped() {
    let mut s = Series::default();
    let st = |ram| StatView {
        ram,
        ..StatView::default()
    };
    assert!(s.push(&st(1), Some(epoch())));
    assert!(!s.push(&st(2), Some(epoch())));
    assert!(s.push(&st(3), Some(epoch() + Duration::from_secs(1))));
    assert_eq!(s.ram, [1.0, 3.0]);
    assert_eq!(s.at.len(), 2);
    // An unstamped reading is a second after the last; the times are dropped.
    assert!(s.push(&st(4), None));
    assert!(s.at.is_empty() && s.len() == 3);
}

#[test]
fn a_series_keeps_four_minutes() {
    let mut s = Series::default();
    for i in 0..(SAMPLES_KEPT as u64 + 10) {
        let v = StatView {
            ram: i as i64,
            cpu: i as f64,
            conns: 1,
            ..StatView::default()
        };
        s.push(&v, Some(epoch() + Duration::from_secs(i)));
    }
    assert_eq!(
        (s.ram.len(), s.cpu.len(), s.conns.len(), s.at.len()),
        (240, 240, 240, 240)
    );
    assert_eq!(s.ram.first(), Some(&10.0));
}
