//! The logs page's index over a ring: kept up line by line it shows what a fresh pass shows, the merged page is in
//! time order however the lines arrive, the counts follow the ring as it lets lines go, and find mode steps through
//! its hits.
#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]

use doze_control::{LogFrame, Timestamp};
use doze_desk::page::filter;
use doze_desk::page::logindex::{LogIndex, LogQuery};
use doze_desk::page::time::Zone;
use doze_desk::{LINES_KEPT, LogRing};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn at(s: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_790_000_000 + s)
}

fn frame(inst: &str, s: u64, line: &str) -> LogFrame {
    LogFrame {
        instance: inst.into(),
        line: line.into(),
        stream: "o".into(),
        run: 1,
        time: Timestamp::from_system_time(at(s)),
    }
}

fn texts(idx: &LogIndex, ring: &LogRing) -> Vec<String> {
    idx.rows(ring, 0..idx.len(), at(10_000), Zone::UTC)
        .into_iter()
        .map(|l| format!("{} {}", l.instance, l.text))
        .collect()
}

#[test]
fn the_merged_page_is_in_time_order_however_lines_arrive() {
    let mut ring = LogRing::new();
    let mut idx = LogIndex::new(LogQuery::default());
    // api's backlog, then db's older one: a reconnect's order.
    for s in [10, 20, 30] {
        ring.push(&frame("api", s, &format!("a{s}")));
    }
    idx.update(&ring, &[]);
    for s in [5, 15, 25, 35] {
        ring.push(&frame("db", s, &format!("d{s}")));
    }
    assert!(idx.update(&ring, &[]));
    assert_eq!(
        texts(&idx, &ring),
        [
            "db d5", "api a10", "db d15", "api a20", "db d25", "api a30", "db d35"
        ]
    );
    assert_eq!(
        idx.instances(),
        ["db", "api"],
        "db's first line is the oldest"
    );
}

#[test]
fn kept_up_line_by_line_it_shows_what_a_fresh_pass_shows() {
    let mut ring = LogRing::new();
    let q = LogQuery {
        level: 1,
        filter: filter::parse("/GET|WARN/"),
        ..LogQuery::default()
    };
    let mut live = LogIndex::new(q.clone());
    for i in 0..3000u64 {
        let inst = ["api", "web", "db"][(i % 3) as usize];
        let line = match i % 7 {
            0 => format!("WARN slow {i}"),
            1 => format!("DEBUG noise {i}"),
            _ => format!("GET /x/{i} 200"),
        };
        ring.push(&frame(inst, i / 3, &line));
        if i % 97 == 0 {
            live.update(&ring, &[]);
        }
    }
    live.update(&ring, &[]);
    let mut fresh = LogIndex::new(q);
    fresh.update(&ring, &[]);
    assert_eq!(texts(&live, &ring), texts(&fresh, &ring));
    assert_eq!(live.counts(), fresh.counts());
    assert_eq!(live.matches(), fresh.matches());
    assert!(texts(&live, &ring).iter().all(|t| !t.contains("noise")));
}

#[test]
fn the_counts_follow_the_ring_as_it_lets_lines_go() {
    let mut ring = LogRing::new();
    let mut idx = LogIndex::new(LogQuery {
        instance: "api".into(),
        ..LogQuery::default()
    });
    let started = Instant::now();
    for i in 0..(LINES_KEPT as u64 + 5_000) {
        let line = if i % 10 == 0 {
            "ERROR boom"
        } else {
            "INFO fine"
        };
        ring.push(&frame("api", i, line));
        if i % 1000 == 0 {
            idx.update(&ring, &[]);
        }
    }
    idx.update(&ring, &[]);
    let took = started.elapsed();
    assert_eq!(ring.len(), LINES_KEPT);
    assert_eq!(idx.len(), LINES_KEPT);
    assert_eq!(idx.counts().iter().sum::<usize>(), LINES_KEPT);
    assert_eq!(idx.counts()[3], LINES_KEPT / 10);
    // The first row is the oldest line held, with its own text.
    let first = idx.rows(&ring, 0..1, at(0), Zone::UTC);
    assert_eq!(first[0].text, "boom");
    // A full pass over the ring, as a change of filter makes.
    let t = Instant::now();
    let mut fresh = LogIndex::new(LogQuery {
        instance: "api".into(),
        filter: filter::parse("/boom/"),
        ..LogQuery::default()
    });
    fresh.update(&ring, &[]);
    eprintln!(
        "105k lines kept up in {took:?}; a 100k pass in {:?}",
        t.elapsed()
    );
    assert_eq!(fresh.len(), LINES_KEPT / 10);
}

#[test]
fn find_mode_keeps_every_line_and_steps_through_the_hits() {
    let mut ring = LogRing::new();
    for (i, l) in ["a", "timeout 1", "b", "c", "timeout 2", "d"]
        .iter()
        .enumerate()
    {
        ring.push(&frame("api", i as u64, l));
    }
    let mut idx = LogIndex::new(LogQuery {
        filter: filter::parse("timeout"),
        find: true,
        ..LogQuery::default()
    });
    idx.update(&ring, &[]);
    assert_eq!((idx.len(), idx.hit_count()), (6, 2));
    assert_eq!(idx.next_hit(None, false), Some((1, 0)));
    assert_eq!(idx.next_hit(Some(1), false), Some((4, 1)));
    assert_eq!(idx.next_hit(Some(4), false), Some((1, 0)), "wraps");
    assert_eq!(idx.next_hit(Some(1), true), Some((4, 1)), "wraps back");
    assert!(idx.is_hit(4) && !idx.is_hit(5));
    // An anchor holds its row while nothing slides out.
    assert_eq!(idx.row_of(idx.anchor(3)), Some(3));
}
