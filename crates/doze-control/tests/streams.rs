//! The client's streams against the fixture daemon playing the Go fixture hubs: subscriptions and their filters,
//! cancel, a stream the daemon ends, refusals, verbs that subscribers hear of, events and logs follow, and a daemon
//! that goes away.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{frame, script, start, start_with, until};
use doze_control::fixture::GONE;
use doze_control::*;
use std::time::Duration;

#[test]
fn a_subscription_gets_only_its_topics_and_names() {
    let Some(d) = start("shop") else { return };
    let conn = d.client().dial().unwrap();
    let sub = Subscription {
        names: vec!["api".into()],
        ..Subscription::to(&[Topic::Instances, Topic::Logs])
    };
    let s = conn.subscribe(&sub).unwrap();
    let snap = frame(&s);
    assert!(snap.snapshot && snap.instances.len() == d.status().instances.len());
    let want = script("shop")
        .logs
        .iter()
        .filter(|l| l.instance == "api")
        .count();
    for _ in 0..want {
        let f = frame(&s);
        assert_eq!(
            (f.topic.as_str(), f.log.map(|l| l.instance)),
            ("logs", Some("api".to_owned()))
        );
    }
    // The script is over and held: nothing more until something happens.
    assert_eq!(s.next_timeout(Duration::from_millis(50)), None);
}

#[test]
fn cancel_ends_the_stream_on_both_sides() {
    let Some(d) = start("shop") else { return };
    let conn = d.client().dial().unwrap();
    let s = conn
        .subscribe(&Subscription::to(&[Topic::Journal]))
        .unwrap();
    let id = s.id();
    for _ in 0..11 {
        assert!(frame(&s).event.is_some());
    }
    until("the stream is held", || d.held() == 1);
    conn.cancel(id).unwrap();
    assert_eq!(
        s.next_timeout(Duration::from_secs(5)),
        Some(StreamEvent::End(None))
    );
    until("the daemon stops the stream", || d.streams() == 0);
    assert!(d.requests().iter().any(|r| r.op == "cancel" && r.id == id));
    // Dropping a live stream cancels it too.
    let s = conn.subscribe(&Subscription::to(&[Topic::Stats])).unwrap();
    until("the second stream is held", || d.held() == 1);
    drop(s);
    until("dropping it stops it", || d.streams() == 0);
    assert_eq!(conn.snapshot().unwrap().stack, "shop");
}

#[test]
fn a_stream_without_hold_ends_by_itself() {
    let mut s = script("shop");
    s.hold = false;
    let Some(d) = start_with("finite", s) else {
        return;
    };
    let conn = d.client().dial().unwrap();
    let stream = conn
        .subscribe(&Subscription::to(&[Topic::Journal]))
        .unwrap();
    let mut n = 0;
    loop {
        match stream.next_blocking() {
            StreamEvent::Frame(_) => n += 1,
            StreamEvent::End(None) => break,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(n, 11);
}

#[test]
fn bad_subscriptions_are_refused_in_go_words() {
    let Some(d) = start("empty") else { return };
    let conn = d.client().dial().unwrap();
    let e = conn.subscribe(&Subscription::default()).unwrap_err();
    assert_eq!(e.to_string(), "subscribe: name at least one topic");
    let e = conn.stream(Request {
        interval: "soon".into(),
        ..Request::subscribe(&Subscription::to(&[Topic::Stats]))
    });
    assert_eq!(
        e.unwrap_err().to_string(),
        "subscribe: \"soon\" is not an interval"
    );
}

#[test]
fn a_verb_changes_the_stack_and_subscribers_hear_of_it() {
    let Some(d) = start("shop") else { return };
    let conn = d.client().dial().unwrap();
    let s = conn
        .subscribe(&Subscription::to(&[Topic::Instances]))
        .unwrap();
    let pinned = |f: &Frame| {
        f.instances
            .iter()
            .find(|i| i.name == "db")
            .map(|i| i.keep_awake)
    };
    let before = pinned(&frame(&s)).unwrap();
    until("the stream is held", || d.held() == 1);
    conn.run_verb(Verb::Pin, "db").unwrap();
    assert_eq!(pinned(&frame(&s)), Some(!before));
    conn.run_verb(Verb::Off, "db").unwrap();
    let db = frame(&s)
        .instances
        .into_iter()
        .find(|i| i.name == "db")
        .unwrap();
    assert!(db.disabled && db.state == "disabled");
}

#[test]
fn events_and_logs_follow_are_streams_too() {
    let Some(d) = start("shop") else { return };
    let conn = d.client().dial().unwrap();
    let logs = conn.follow_logs(&["api".to_owned()]).unwrap();
    let f = frame(&logs);
    assert_eq!(f.log.unwrap().instance, "api");
    let events = conn.events().unwrap();
    until("both are held", || d.held() == 2);
    let changed = InstanceView {
        name: "web".into(),
        state: "idle".into(),
        ..InstanceView::default()
    };
    d.push(&Frame {
        topic: "instances".into(),
        instances: vec![changed.clone()],
        ..Frame::default()
    });
    assert_eq!(frame(&events).instances, vec![changed]);
}

#[test]
fn a_daemon_that_goes_away_ends_its_streams() {
    let Some(d) = start("shop") else { return };
    let conn = d.client().dial().unwrap();
    let s = conn
        .subscribe(&Subscription::to(&[Topic::Journal]))
        .unwrap();
    for _ in 0..11 {
        frame(&s);
    }
    until("the stream is held", || d.held() == 1);
    d.hang_up();
    match s.next_timeout(Duration::from_secs(5)) {
        Some(StreamEvent::End(Some(e))) => assert_eq!(e, GONE),
        Some(StreamEvent::Lost(_)) => {}
        other => panic!("{other:?}"),
    }
    until("the connection is closed", || conn.closed().is_some());
    assert!(conn.snapshot().unwrap_err().is_unreachable());
}
