//! The client against the fixture daemon on a real socket, playing the Go fixture hubs' scripts: hello, unary calls
//! and their errors, refusals in words, multiplexing, logs, data actions. Streams are in `streams.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{frame, script, start, start_with};
use doze_control::fixture::temp_socket;
use doze_control::*;
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

#[test]
fn hello_then_status() {
    let Some(d) = start("shop") else { return };
    let conn = d.client().named("app").dial().unwrap();
    let snap = conn.snapshot().unwrap();
    assert_eq!(snap.stack, "shop");
    assert_eq!(snap.idle_timeout.to_std(), Duration::from_secs(300));
    assert_eq!(snap.instances.len(), d.status().instances.len());
    let first = &d.requests()[0];
    assert_eq!(
        first.hello,
        Some(Hello {
            proto: 2,
            client: "app".into()
        })
    );
    assert!(d.client().available());
}

#[test]
fn errors_come_back_with_their_words_and_kind() {
    let mut s = script("shop");
    s.fail.push(fixture::Fail {
        key: "wake missing".into(),
        error: "instance \"missing\" is not declared".into(),
        code: "not_found".into(),
    });
    s.fail.push(fixture::Fail {
        key: "restart".into(),
        error: "no".into(),
        code: String::new(),
    });
    let Some(d) = start_with("errors", s) else {
        return;
    };
    let conn = d.client().dial().unwrap();
    let e = conn.run_verb(Verb::Wake, "missing").unwrap_err();
    assert_eq!(
        (e.to_string().as_str(), e.code()),
        (
            "instance \"missing\" is not declared",
            Some(fault::NOT_FOUND)
        )
    );
    assert_eq!(
        conn.run_verb(Verb::Restart, "api").unwrap_err().code(),
        None
    );
    let e = conn
        .call(Request {
            op: "frobnicate".into(),
            ..Request::default()
        })
        .unwrap_err();
    assert_eq!(e.to_string(), "unknown op: frobnicate");
    assert!(conn.run_verb(Verb::Wake, "api").is_ok());
}

#[test]
fn an_old_or_newer_client_is_told_in_words() {
    let Some(d) = start("empty") else { return };
    for (hello, words) in [
        ("{\"op\":\"status\"}", "no protocol version"),
        ("{\"hello\":{\"proto\":3}}", "protocol 3"),
    ] {
        let mut s = doze_os::local::LocalStream::connect(d.path()).unwrap();
        writeln!(s, "{hello}").unwrap();
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        let f: Frame = serde_json::from_str(&line).unwrap();
        assert!(
            f.error.contains(words) && f.error.contains("doze down"),
            "{}",
            f.error
        );
    }
}

#[test]
fn nothing_listening_is_a_connect_error() {
    let c = Client::socket(temp_socket("nobody"));
    assert!(!c.available());
    let e = c.dial().unwrap_err();
    assert!(matches!(e, Error::Connect(_)) && e.is_unreachable(), "{e}");
}

#[test]
fn one_connection_carries_a_stream_and_calls_at_once() {
    let Some(d) = start("shop") else { return };
    let conn = d.client().dial().unwrap();
    let stream = conn.subscribe(&Subscription::to(&[Topic::Stats])).unwrap();
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let c = conn.clone();
            std::thread::spawn(move || c.snapshot().map(|s| s.stack))
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap().unwrap(), "shop");
    }
    assert_eq!(frame(&stream).topic, "stats");
}

#[test]
fn logs_by_run_tail_and_name() {
    let Some(d) = start("crash") else { return };
    let conn = d.client().dial().unwrap();
    let all = conn
        .logs(&LogQuery {
            instance: "api".into(),
            ..LogQuery::default()
        })
        .unwrap();
    assert!(all.len() > 2);
    let tail = conn
        .logs(&LogQuery {
            instance: "api".into(),
            tail: 2,
            ..LogQuery::default()
        })
        .unwrap();
    assert_eq!(tail, all[all.len() - 2..]);
    let run = all.last().unwrap().run;
    let only = conn
        .logs(&LogQuery {
            instance: "api".into(),
            run,
            ..LogQuery::default()
        })
        .unwrap();
    assert!(!only.is_empty() && only.iter().all(|l| l.run == run));
    let e = conn
        .logs(&LogQuery {
            instance: "nope".into(),
            ..LogQuery::default()
        })
        .unwrap_err();
    assert_eq!(e.to_string(), "instance \"nope\" is not declared");
}

#[test]
fn admin_and_resources() {
    let mut s = script("modules");
    s.admin_result = "purged 3".into();
    let Some(d) = start_with("admin", s) else {
        return;
    };
    let conn = d.client().dial().unwrap();
    assert_eq!(
        conn.admin("queue", "purge", "orders", "").unwrap(),
        "purged 3"
    );
    assert_eq!(conn.resources("queue").unwrap(), (vec![], vec![]));
    let r = d.requests();
    let admin = r.iter().find(|r| r.op == "admin").unwrap();
    assert_eq!(
        (
            admin.db.as_str(),
            admin.action.as_str(),
            admin.resource.as_str()
        ),
        ("queue", "purge", "orders")
    );
}
