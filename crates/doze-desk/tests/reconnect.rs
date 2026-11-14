//! The connection's state machine: Connecting, Live, Down and back, as the rule and on a real socket.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Home, daemon_at, epoch, until, wait};
use doze_control::fixture::{Script, temp_socket};
use doze_control::{Event, FollowEvent, Frame, Op, Response, Verb};
use doze_desk::{Conn, Error, Hub, Link};
use std::time::Duration;

#[test]
fn the_rule() {
    let now = epoch();
    let mut l = Link::default();
    assert_eq!(l.conn, Conn::Connecting);
    assert!(
        !l.on(&FollowEvent::Connecting { attempt: 1 }, now),
        "connecting while connecting changes nothing"
    );

    let down = FollowEvent::Down {
        error: "connection refused".into(),
        retry_in: Duration::from_secs(3),
    };
    assert!(l.on(&down, now));
    assert_eq!(l.conn, Conn::Down);
    assert_eq!(l.retry_at, Some(now + Duration::from_secs(3)));
    assert_eq!(l.error, "connection refused");

    // Each new attempt keeps the page saying "retrying in …", not "connecting".
    assert!(!l.on(&FollowEvent::Connecting { attempt: 2 }, now));
    assert_eq!(l.conn, Conn::Down);

    assert!(l.on(&FollowEvent::Live, now));
    assert_eq!(
        (l.conn, l.retry_at, l.error.as_str()),
        (Conn::Live, None, "")
    );
    assert_eq!(l.last_frame, Some(now));

    let later = now + Duration::from_secs(1);
    assert!(
        l.on(&FollowEvent::Frame(Box::default()), later),
        "a frame moves the last frame's time"
    );
    assert_eq!(l.last_frame, Some(later));
}

#[test]
fn down_while_nothing_listens_then_live_when_the_daemon_comes() {
    let sock = temp_socket("late");
    let home = Home::new();
    let key = home.key("late-1");
    home.put_socket(&key);
    home.route(&key, &sock);
    let hub = Hub::start(home.options());
    wait("found", || hub.stack(&key).is_some());
    let first = hub.stack(&key).unwrap().link.conn;
    assert!(matches!(first, Conn::Connecting | Conn::Down), "{first:?}");
    let s = until(&hub, &key, "down", |s| s.link.conn == Conn::Down);
    assert!(s.link.retry_at.is_some());
    assert!(matches!(
        hub.verb(&key, Verb::Wake, "api"),
        Err(Error::NotResponding)
    ));

    let script = Script {
        status: Response {
            ok: true,
            stack: "late".into(),
            ..Response::default()
        },
        hold: true,
        ..Script::default()
    };
    let Some(_d) = daemon_at(&sock, script) else {
        return;
    };
    let s = until(&hub, &key, "live", |s| {
        s.link.conn == Conn::Live && s.name == "late"
    });
    assert!(s.link.retry_at.is_none());
}

#[test]
fn a_numbered_journal_resumes_after_the_last_event() {
    let sock = temp_socket("resume");
    let ev = |seq: u64| {
        Box::new(Frame {
            topic: "journal".into(),
            event: Some(Event {
                seq,
                kind: "wake".into(),
                instance: "api".into(),
                ..Event::default()
            }),
            ..Frame::default()
        })
    };
    let script = Script {
        status: Response {
            ok: true,
            stack: "shop".into(),
            ..Response::default()
        },
        hold: true,
        steps: (1..=3)
            .map(|n| doze_control::fixture::Step::Frame(ev(n)))
            .collect(),
        ..Script::default()
    };
    let Some(d) = daemon_at(&sock, script) else {
        return;
    };
    let home = Home::new();
    let key = home.stack("shop-1", &d);
    let hub = Hub::start(home.options());
    until(&hub, &key, "three events", |s| s.events.len() == 3);

    d.hang_up();
    wait("subscribed again", || {
        d.requests()
            .iter()
            .filter(|r| r.known_op() == Some(Op::Subscribe))
            .count()
            == 2
    });
    let again = d
        .requests()
        .into_iter()
        .rfind(|r| r.known_op() == Some(Op::Subscribe))
        .unwrap();
    assert_eq!(
        again.since, 3,
        "the journal resumes after the last event delivered"
    );
    until(&hub, &key, "live", |s| s.link.conn == Conn::Live);
    std::thread::sleep(Duration::from_millis(50));
    let seqs: Vec<u64> = hub
        .stack(&key)
        .unwrap()
        .events
        .iter()
        .map(|e| e.seq)
        .collect();
    assert_eq!(seqs, [1, 2, 3], "the replay is not kept twice");
}

#[test]
fn a_console_whose_daemon_goes_says_so_and_comes_back() {
    let sock = temp_socket("console");
    let Some(d) = daemon_at(&sock, common::script("modules")) else {
        return;
    };
    let home = Home::new();
    let key = home.stack("p-1", &d);
    let hub = Hub::start(home.options());
    let _w = hub.watch_console(&key, "db");
    until(&hub, &key, "db's console", |s| {
        s.consoles
            .get("db")
            .is_some_and(|c| c.tables.contains_key("sessions"))
    });
    d.stop();
    let s = until(&hub, &key, "the console's error", |s| {
        s.consoles.get("db").is_some_and(|c| !c.err.is_empty())
    });
    let err = s.consoles.get("db").unwrap().err.clone();
    assert!(
        [
            "the daemon closed the connection",
            "the daemon is not reachable"
        ]
        .contains(&err.as_str()),
        "{err}"
    );
    until(&hub, &key, "the retries say why", |s| {
        s.consoles
            .get("db")
            .is_some_and(|c| c.err == "the daemon is not reachable")
    });
    let _back = daemon_at(&sock, common::script("modules")).unwrap();
    until(&hub, &key, "the console back, its error gone", |s| {
        s.link.is_live()
            && s.consoles
                .get("db")
                .is_some_and(|c| c.err.is_empty() && c.spec.is_some())
    });
}
