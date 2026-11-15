//! The reconnecting follower against the fixture daemon: it comes back after the daemon restarts, resumes the
//! journal where it left off, waits 500 ms doubling to 5 s between attempts, follows new topics without hanging up,
//! and stops when told.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{script, start_with, until};
use doze_control::fixture::{FixtureDaemon, temp_socket};
use doze_control::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);

/// A follower's events, each stamped the moment it arrived (a thread blocks on the channel, so the stamps are not
/// late by a polling interval).
struct Watch {
    follow: Follow,
    rx: mpsc::Receiver<(Instant, FollowEvent)>,
}

impl Watch {
    fn new(follow: Follow) -> Watch {
        let (tx, rx) = mpsc::channel();
        let events = follow.events().clone();
        std::thread::spawn(move || {
            while let Ok(e) = events.recv_blocking() {
                if tx.send((Instant::now(), e)).is_err() {
                    return;
                }
            }
        });
        Watch { follow, rx }
    }

    fn stamped(&self) -> (Instant, FollowEvent) {
        self.rx
            .recv_timeout(WAIT)
            .unwrap_or_else(|e| panic!("no follow event in {WAIT:?}: {e}"))
    }

    fn next(&self) -> FollowEvent {
        self.stamped().1
    }

    /// The next event that is not a topic frame.
    fn next_state(&self) -> FollowEvent {
        loop {
            match self.next() {
                FollowEvent::Frame(_) => {}
                e => return e,
            }
        }
    }
}

fn instances_and_journal() -> Subscription {
    Subscription::to(&[Topic::Instances, Topic::Journal])
}

#[test]
fn backoff_doubles_from_500ms_to_5s() {
    let b = Backoff::default();
    let ms: Vec<u128> = (0..8).map(|n| b.delay(n).as_millis()).collect();
    assert_eq!(ms, [0, 500, 1000, 2000, 4000, 5000, 5000, 5000]);
    assert_eq!(b.delay(u32::MAX), Duration::from_secs(5));
}

#[test]
fn it_comes_back_after_the_daemon_restarts_and_resumes_the_journal() {
    let Some(d) = start_with("restart", script("shop")) else {
        return;
    };
    let path = d.path().to_path_buf();
    let w = Watch::new(Client::socket(&path).follow(instances_and_journal(), Backoff::default()));
    assert_eq!(w.next(), FollowEvent::Connecting { attempt: 1 });
    assert_eq!(w.next(), FollowEvent::Live);
    let FollowEvent::Frame(snap) = w.next() else {
        panic!("no snapshot")
    };
    assert!(snap.snapshot);
    until("the stream is held", || d.held() == 1);
    // Something happens after the script: the follower remembers the journal event.
    let event = Event {
        seq: 42,
        kind: "wake".into(),
        instance: "db".into(),
        ..Event::default()
    };
    d.push(&Frame {
        topic: "journal".into(),
        event: Some(event),
        ..Frame::default()
    });
    while !matches!(w.next(), FollowEvent::Frame(f) if f.event.as_ref().is_some_and(|e| e.seq == 42))
    {
    }

    d.stop();
    let (down_at, down) = w.stamped();
    let FollowEvent::Down { retry_in, .. } = down else {
        panic!("not down: {down:?}")
    };
    assert_eq!(retry_in, Duration::from_millis(500));
    // Back after the restart: a new connection, a new snapshot, and the journal asked for from after event 42.
    let d = FixtureDaemon::start(&path, script("shop")).unwrap();
    let (at, again) = w.stamped();
    assert_eq!(again, FollowEvent::Connecting { attempt: 1 });
    // Both stamps are taken as a watching thread receives the events, so a slow runner can read the Down late; the
    // wait itself is 500 ms (retry_in above), and the gap measured here allows 50 ms of that lateness.
    assert!(at - down_at >= Duration::from_millis(450));
    assert_eq!(w.next_state(), FollowEvent::Live);
    let FollowEvent::Frame(snap) = w.next() else {
        panic!("no snapshot")
    };
    assert!(snap.snapshot);
    let sub = d
        .requests()
        .into_iter()
        .find(|r| r.op == "subscribe")
        .unwrap();
    assert_eq!(
        (sub.since, sub.topics),
        (42, vec!["instances".to_owned(), "journal".to_owned()])
    );
    w.follow.stop();
    until("the follower hangs up", || d.streams() == 0);
}

/// How late the watching thread may stamp an event: the follower sleeps at least the wait, but the stamps are taken
/// on another thread.
const JITTER: Duration = Duration::from_millis(10);

/// How late an attempt may come: never early, but a busy shared CI runner adds to the wait (macOS runners were 118 ms
/// and 115 ms late).
const LATE: Duration = Duration::from_millis(400);

/// The gaps between attempts, from the moment each `Down` arrived to the next `Connecting`.
fn gaps(w: &Watch, n: usize) -> Vec<(Duration, Duration)> {
    let mut out = Vec::new();
    let mut down: Option<(Instant, Duration)> = None;
    while out.len() < n {
        match w.stamped() {
            (at, FollowEvent::Down { retry_in, .. }) => down = Some((at, retry_in)),
            (at, FollowEvent::Connecting { .. }) => {
                if let Some((then, wait)) = down.take() {
                    out.push((wait, at - then));
                }
            }
            (_, e) => panic!("{e:?}"),
        }
    }
    out
}

#[test]
fn it_backs_off_between_attempts() {
    let path = temp_socket("nobody");
    let b = Backoff {
        first: Duration::from_millis(40),
        max: Duration::from_millis(200),
    };
    let w = Watch::new(Client::socket(&path).follow(instances_and_journal(), b));
    let got = gaps(&w, 5);
    let waits: Vec<u128> = got.iter().map(|g| g.0.as_millis()).collect();
    assert_eq!(waits, [40, 80, 160, 200, 200]);
    for (wait, gap) in got {
        assert!(
            gap + JITTER >= wait && gap < wait + LATE,
            "gap {gap:?} for a wait of {wait:?}"
        );
    }
    // And with the default back-off the first two waits are 500 ms and 1 s.
    let w = Watch::new(Client::socket(&path).follow(instances_and_journal(), Backoff::default()));
    let got = gaps(&w, 2);
    assert_eq!(
        got.iter().map(|g| g.0).collect::<Vec<_>>(),
        [Duration::from_millis(500), Duration::from_secs(1)]
    );
    for (wait, gap) in got {
        assert!(
            gap + JITTER >= wait && gap < wait + LATE,
            "gap {gap:?} for a wait of {wait:?}"
        );
    }
}

#[test]
fn it_follows_new_topics_on_the_same_connection() {
    let Some(d) = start_with("resub", script("shop")) else {
        return;
    };
    let w = Watch::new(
        d.client()
            .follow(Subscription::to(&[Topic::Journal]), Backoff::default()),
    );
    assert_eq!(w.next_state(), FollowEvent::Connecting { attempt: 1 });
    assert_eq!(w.next_state(), FollowEvent::Live);
    until("the journal stream is held", || d.held() == 1);
    w.follow.resubscribe(Subscription::to(&[Topic::Stats]));
    while !matches!(w.next(), FollowEvent::Frame(f) if f.topic == "stats") {}
    assert_eq!(d.accepted(), 1);
    let ops: Vec<String> = d.requests().into_iter().map(|r| r.op).collect();
    assert_eq!(ops, ["", "subscribe", "cancel", "subscribe"]);
    until("only the new stream runs", || d.streams() == 1);
}

#[test]
fn stopping_closes_the_events_and_the_connection() {
    let Some(d) = start_with("stop", script("empty")) else {
        return;
    };
    let f = d
        .client()
        .follow(instances_and_journal(), Backoff::default());
    let events = f.events().clone();
    let w = Watch::new(f);
    assert_eq!(w.next_state(), FollowEvent::Connecting { attempt: 1 });
    assert_eq!(w.next_state(), FollowEvent::Live);
    drop(w);
    until("the events close", || events.is_closed());
    until("the daemon's stream ends", || d.streams() == 0);
}
