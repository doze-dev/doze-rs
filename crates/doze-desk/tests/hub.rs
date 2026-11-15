//! The Go hub's tests (`internal/desk/hub/hub_test.go`), against fixture daemons on real sockets.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Home, daemon, daemon_at, until, wait};
use doze_control::fixture::{Script, Step, temp_socket};
use doze_control::{Event, Frame, InstanceView, Op, Response, Verb};
use doze_desk::{Conn, Error, Hub, Parts, name_from_key};
use std::path::Path;
use std::time::Duration;

fn inst(name: &str, kind: &str, state: &str) -> InstanceView {
    InstanceView {
        name: name.into(),
        kind: kind.into(),
        state: state.into(),
        ..InstanceView::default()
    }
}

fn shop_status() -> Response {
    Response {
        ok: true,
        stack: "shop".into(),
        dir: "/code/shop".into(),
        instances: vec![inst("db", "module", "active"), inst("api", "app", "reaped")],
        ..Response::default()
    }
}

#[test]
fn running_stacks_are_found_and_followed() {
    let script = Script {
        status: shop_status(),
        hold: true,
        steps: vec![
            Step::Frame(Box::new(Frame {
                topic: "instances".into(),
                instances: vec![inst("api", "app", "booting")],
                ..Frame::default()
            })),
            Step::Frame(Box::new(Frame {
                topic: "journal".into(),
                event: Some(Event {
                    seq: 7,
                    kind: "wake".into(),
                    instance: "api".into(),
                    ..Event::default()
                }),
                ..Frame::default()
            })),
        ],
        ..Script::default()
    };
    let Some(d) = daemon("found", script) else {
        return;
    };
    let home = Home::new();
    let key = home.stack("shop-1", &d);
    let hub = Hub::start(home.options());
    let s = until(&hub, &key, "live with the event", |s| {
        s.link.conn == Conn::Live && s.events.len() == 1 && s.name == "shop"
    });
    assert_eq!((s.dir.as_str(), s.running), ("/code/shop", true));
    let names: Vec<_> = s
        .instances
        .iter()
        .map(|v| (v.name.as_str(), v.state.as_str()))
        .collect();
    assert_eq!(
        names,
        [("api", "booting"), ("db", "active")],
        "instances after the frame"
    );
    assert_eq!(s.events.first().map(|e| e.seq), Some(7));

    hub.verb(&key, Verb::Wake, "api").unwrap();
    let wakes: Vec<_> = d
        .requests()
        .into_iter()
        .filter(|r| r.known_op() == Some(Op::Wake))
        .collect();
    assert_eq!(wakes.len(), 1);
    assert_eq!(wakes.first().map(|r| r.db.as_str()), Some("api"));
}

#[test]
fn a_daemon_that_stops_answering_is_down_then_retried() {
    let sock = temp_socket("down");
    let script = Script {
        status: shop_status(),
        hold: true,
        ..Script::default()
    };
    let Some(d) = daemon_at(&sock, script.clone()) else {
        return;
    };
    let home = Home::new();
    let key = home.stack("shop-1", &d);
    let hub = Hub::start(home.options());
    until(&hub, &key, "live", |s| s.link.conn == Conn::Live);

    d.stop();
    let s = until(&hub, &key, "down", |s| s.link.conn == Conn::Down);
    assert!(
        s.link.retry_at.is_some(),
        "a down stack says when it retries"
    );
    assert!(!s.link.error.is_empty(), "and why");
    assert!(matches!(
        hub.verb(&key, Verb::Sleep, "db"),
        Err(Error::NotResponding)
    ));

    // The daemon comes back; the hub's retry finds it without being told.
    let _back = daemon_at(&sock, script).unwrap();
    let s = until(&hub, &key, "live again", |s| s.link.conn == Conn::Live);
    assert!(s.link.retry_at.is_none() && s.link.error.is_empty());
}

#[test]
fn stopped_stacks_come_from_the_project_record() {
    let home = Home::new();
    let key = home.record(
        "old-1",
        r#"{"schema":1,"config":"/code/old/doze.hcl","name":"old","last_down":"1970-01-01T00:01:40Z","last_set":["api"]}"#,
    );
    let hub = Hub::new(home.options());
    hub.refresh();
    let st = hub.stacks();
    assert_eq!(st.len(), 1);
    let s = st.first().unwrap();
    assert!(!s.running);
    assert_eq!((s.name.as_str(), s.dir.as_str()), ("old", "/code/old"));
    assert_eq!(s.last_set, ["api"]);
    assert_eq!(
        s.last_down,
        Some(std::time::UNIX_EPOCH + Duration::from_secs(100))
    );

    // The first refresh is reported, once.
    let b = hub.updates().recv_blocking().unwrap();
    assert!(b.hub.contains(Parts::LIST), "{b:?}");
    hub.refresh();
    std::thread::sleep(Duration::from_millis(60));
    assert!(
        hub.updates().try_recv().is_err(),
        "an unchanged refresh is not reported"
    );

    assert!(matches!(
        hub.verb(&key, Verb::Wake, ""),
        Err(Error::NotRunning)
    ));
}

#[test]
fn a_stack_whose_socket_vanishes_is_dropped() {
    let Some(d) = daemon(
        "vanish",
        Script {
            status: shop_status(),
            hold: true,
            ..Script::default()
        },
    ) else {
        return;
    };
    let home = Home::new();
    let key = home.stack("shop-1", &d);
    let hub = Hub::start(home.options());
    wait("the stack is found", || hub.stacks().len() == 1);
    home.remove_socket(&key);
    wait("the stack is dropped", || hub.stacks().is_empty());
    // Its thread is gone with it: the daemon's stream is cancelled.
    wait("the stream ends", || d.streams() == 0);
}

#[test]
fn a_stack_seen_running_and_recorded_keeps_its_record() {
    let Some(d) = daemon(
        "record",
        Script {
            status: shop_status(),
            hold: true,
            ..Script::default()
        },
    ) else {
        return;
    };
    let home = Home::new();
    let key = home.stack("shop-1", &d);
    home.record(
        "shop-1",
        r#"{"schema":1,"config":"/code/shop/doze.hcl","name":"shop","last_set":["api","worker"]}"#,
    );
    let hub = Hub::start(home.options());
    let s = until(&hub, &key, "the record is read", |s| !s.last_set.is_empty());
    assert!(s.running, "a running stack is not also listed as stopped");
    assert_eq!(s.last_set, ["api", "worker"]);
    assert_eq!(hub.stacks().len(), 1);
}

#[test]
fn a_daemon_that_says_no_name_is_named_by_its_folder() {
    let Some(d) = daemon(
        "noname",
        Script {
            hold: true,
            ..Script::default()
        },
    ) else {
        return;
    };
    let home = Home::new();
    let key = home.stack("ai-interview-bed9cc55", &d);
    let hub = Hub::start(home.options());
    let s = until(&hub, &key, "live", |s| s.link.is_live());
    assert_eq!(s.name, "ai-interview");
}

#[test]
fn names_from_keys() {
    for (k, want) in [
        ("ai-interview-bed9cc55", "ai-interview"),
        ("/h/.doze/projects/shop-918fa699", "shop"),
        ("shop-918fa699", "shop"),
        ("odd", "odd"),
        ("x-notahash", "x-notahash"),
        ("-918fa699", "-918fa699"),
        ("shop-918FA699", "shop-918FA699"),
    ] {
        assert_eq!(name_from_key(Path::new(k)), want, "{k}");
    }
}

#[test]
fn verbs_and_actions_go_on_a_connection_of_their_own() {
    let script = Script {
        status: shop_status(),
        hold: true,
        admin_result: "purged 3 messages".into(),
        ..Script::default()
    };
    let Some(d) = daemon("actions", script) else {
        return;
    };
    let home = Home::new();
    let key = home.stack("shop-1", &d);
    let hub = Hub::start(home.options());
    until(&hub, &key, "live", |s| {
        s.link.conn == Conn::Live && s.name == "shop"
    });
    let before = d.accepted();
    assert_eq!(
        hub.admin(&key, "events", "purge", "orders", "").unwrap(),
        "purged 3 messages"
    );
    hub.verb(&key, Verb::Sleep, "db").unwrap();
    assert_eq!(d.accepted(), before + 2, "one short-lived connection each");
    assert_eq!(d.streams(), 1, "the stream was not touched");
}
