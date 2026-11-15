//! Coalescing: the rule with made-up instants, and the hub's batches while a fixture floods it.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{following, wait};
use doze_control::fixture::FixtureDaemon;
use doze_control::{Frame, LogFrame, Topic};
use doze_desk::{Batch, Changed, Coalescer, FRAME, Hub, Parts, Policy, StackKey, Watch};
use std::time::{Duration, Instant};

fn key(k: &str) -> StackKey {
    StackKey::new(k)
}

#[test]
fn changes_within_a_frame_are_one_batch() {
    let mut c = Coalescer::new(Policy::default());
    let t0 = Instant::now();
    assert!(c.take(t0).is_none(), "nothing changed");
    c.push(Changed::stack(&key("a"), Parts::LOGS));
    let first = c.take(t0).unwrap();
    assert_eq!(
        first.parts(&key("a")),
        Parts::LOGS,
        "the first change goes at once"
    );

    c.push(Changed::stack(&key("a"), Parts::LOGS));
    c.push(Changed::stack(&key("a"), Parts::STATS));
    c.push(Changed::stack(&key("b"), Parts::CONN));
    c.push(Changed::list());
    assert!(
        c.take(t0 + Duration::from_millis(5)).is_none(),
        "not twice in a frame"
    );
    assert_eq!(c.due(), Some(t0 + FRAME));
    let b = c.take(t0 + FRAME).unwrap();
    assert_eq!(b.parts(&key("a")), Parts::LOGS | Parts::STATS);
    assert_eq!(b.parts(&key("b")), Parts::CONN);
    assert!(b.hub.contains(Parts::LIST));
    assert!(
        c.take(t0 + 3 * FRAME).is_none() && c.due().is_none(),
        "and nothing is left"
    );
}

#[test]
fn nothing_is_said_while_hidden_and_all_of_it_when_shown() {
    let mut c = Coalescer::new(Policy::default());
    let t0 = Instant::now();
    c.set_visible(false);
    for _ in 0..1000 {
        c.push(Changed::stack(&key("a"), Parts::LOGS));
    }
    c.push(Changed::stack(&key("b"), Parts::INSTANCES));
    assert!(c.due().is_none());
    assert!(c.take(t0 + Duration::from_secs(60)).is_none());
    c.set_visible(true);
    let b = c.take(t0 + Duration::from_secs(60)).unwrap();
    assert_eq!(b.stacks.len(), 2);
    assert_eq!(b.parts(&key("b")), Parts::INSTANCES);
}

#[test]
fn a_hidden_rate_can_be_asked_for() {
    let mut c = Coalescer::new(Policy {
        every: FRAME,
        hidden: Some(Duration::from_secs(1)),
    });
    let t0 = Instant::now();
    c.push(Changed::list());
    assert!(c.take(t0).is_some());
    c.set_visible(false);
    c.push(Changed::list());
    assert!(c.take(t0 + Duration::from_millis(500)).is_none());
    assert!(c.take(t0 + Duration::from_secs(1)).is_some());
}

#[test]
fn a_batch_the_ui_has_not_taken_keeps_merging() {
    let mut c = Coalescer::new(Policy::default());
    let t0 = Instant::now();
    c.push(Changed::stack(&key("a"), Parts::LOGS));
    let b = c.take(t0).unwrap();
    c.push(Changed::stack(&key("a"), Parts::STATS));
    c.put_back(b);
    let b = c.take(t0 + FRAME).unwrap();
    assert_eq!(b.parts(&key("a")), Parts::LOGS | Parts::STATS);
}

fn line(text: &str) -> Frame {
    Frame {
        topic: "logs".into(),
        log: Some(LogFrame {
            instance: "api".into(),
            line: text.into(),
            ..LogFrame::default()
        }),
        ..Frame::default()
    }
}

/// The shop fixture with its logs watched and the backlog in, and the batches so far taken.
fn flooding() -> Option<(common::Home, FixtureDaemon, Hub, StackKey, Watch)> {
    let (home, d, hub, key) = following("shop")?;
    let logs = hub.watch(&key, Topic::Logs);
    wait("the backlog", || hub.logs(&key, "").len() == 23);
    while hub.updates().try_recv().is_ok() {}
    Some((home, d, hub, key, logs))
}

/// The hub under a flood of log lines: thousands of folds, at most one batch a frame.
#[test]
fn a_flood_reaches_the_ui_at_most_once_a_frame() {
    let Some((_home, d, hub, key, _logs)) = flooding() else {
        return;
    };
    let flood = 3_000;
    let frames: Vec<Frame> = (0..flood)
        .map(|i| line(&format!("GET /flood/{i} 200")))
        .collect();
    let started = Instant::now();
    for f in &frames {
        d.push(f);
    }
    let mut batches: Vec<Batch> = Vec::new();
    while hub.logs(&key, "").len() < 23 + flood {
        batches.extend(hub.updates().try_recv());
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the flood never arrived"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    std::thread::sleep(3 * FRAME);
    batches.extend(hub.updates().try_recv());
    let elapsed = started.elapsed();
    assert!(!batches.is_empty());
    assert!(batches.iter().all(|b| b.parts(&key).contains(Parts::LOGS)));
    let most = (elapsed.as_millis() / FRAME.as_millis()) as usize + 2;
    eprintln!(
        "{flood} lines in {elapsed:?}: {} batches (at most {most})",
        batches.len()
    );
    assert!(
        batches.len() <= most,
        "{} batches in {elapsed:?}",
        batches.len()
    );
}

/// Hidden: the lines are folded, and the UI hears nothing until it is shown.
#[test]
fn a_hidden_ui_hears_nothing_until_it_is_shown() {
    let Some((_home, d, hub, key, _logs)) = flooding() else {
        return;
    };
    hub.set_visible(false);
    std::thread::sleep(3 * FRAME);
    while hub.updates().try_recv().is_ok() {}
    d.push(&line("while hidden"));
    wait("folded while hidden", || hub.logs(&key, "").len() == 24);
    std::thread::sleep(5 * FRAME);
    assert!(
        hub.updates().try_recv().is_err(),
        "nothing is said while hidden"
    );
    hub.set_visible(true);
    let b = hub.updates().recv_blocking().unwrap();
    assert!(b.parts(&key).contains(Parts::LOGS));
}
