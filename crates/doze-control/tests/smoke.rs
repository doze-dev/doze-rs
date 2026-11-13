//! Against a real Go daemon, read-only ops only. Skipped unless `DOZE_CONTROL_SMOKE` names its control socket
//! (`<DOZE_HOME>/projects/<key>/run/doze.admin.sock`); start one on a throwaway stack in a temp `DOZE_HOME`, never a
//! stack you care about. The ops it sends: hello, status, logs, resources, subscribe (then cancel), events (then
//! cancel), logs with follow (then cancel), an unknown op, and a subscribe the daemon refuses.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use doze_control::*;
use doze_os::local::LocalStream;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn socket() -> Option<PathBuf> {
    let p = std::env::var_os("DOZE_CONTROL_SMOKE").map(PathBuf::from);
    if p.is_none() {
        eprintln!("skipped: set DOZE_CONTROL_SMOKE to a Go daemon's control socket");
    }
    p
}

/// The next event that is a frame, within a few seconds.
fn frame(s: &Stream) -> Frame {
    match s.next_timeout(Duration::from_secs(5)) {
        Some(StreamEvent::Frame(f)) => *f,
        other => panic!("wanted a frame, got {other:?}"),
    }
}

#[test]
fn the_client_speaks_to_a_go_daemon() {
    let Some(path) = socket() else { return };
    let client = Client::socket(&path).named("doze-control-smoke");
    assert!(client.available());
    let conn = client.dial().unwrap();
    let snap = conn.snapshot().unwrap();
    assert!(!snap.instances.is_empty() && !snap.dir.is_empty() && !snap.loaded.is_zero());
    let first = &snap.instances[0].name;
    conn.logs(&LogQuery {
        instance: first.clone(),
        tail: 3,
        ..LogQuery::default()
    })
    .unwrap();
    conn.resources(first).unwrap();
    let e = conn
        .call(Request {
            op: "frobnicate".into(),
            ..Request::default()
        })
        .unwrap_err();
    assert_eq!(e.to_string(), "unknown op: frobnicate");
    let e = conn
        .logs(&LogQuery {
            instance: "no-such-instance".into(),
            ..LogQuery::default()
        })
        .unwrap_err();
    eprintln!("an unknown instance: {e} (code {:?})", e.code());
    assert!(
        conn.subscribe(&Subscription::default())
            .unwrap_err()
            .to_string()
            .contains("at least one topic")
    );

    // A subscription: the snapshot first, the journal from the start, stats at an interval; then cancelled.
    let sub = Subscription {
        since: JOURNAL_FROM_START,
        interval: Some(Duration::from_millis(250)),
        ..Subscription::to(&[Topic::Instances, Topic::Journal, Topic::Stats])
    };
    let s = conn.subscribe(&sub).unwrap();
    // The daemon sends its recent stats first, then the instances snapshot.
    let deadline = Instant::now() + Duration::from_secs(5);
    let (mut seen, mut snapshot) = (std::collections::BTreeSet::new(), false);
    while !(snapshot && seen.contains("stats")) && Instant::now() < deadline {
        let f = frame(&s);
        snapshot |=
            f.snapshot && f.topic == "instances" && f.instances.len() == snap.instances.len();
        seen.insert(f.topic);
    }
    assert!(snapshot && seen.contains("stats"), "{seen:?}");
    let id = s.id();
    conn.cancel(id).unwrap();
    loop {
        match s.next_timeout(Duration::from_secs(5)) {
            Some(StreamEvent::Frame(_)) => {}
            Some(StreamEvent::End(None)) => break,
            other => panic!("after cancel: {other:?}"),
        }
    }
    // Events and a logs follow open and cancel on the same connection.
    drop(conn.events().unwrap());
    drop(conn.follow_logs(&[]).unwrap());
    assert_eq!(conn.snapshot().unwrap().dir, snap.dir);
    eprintln!(
        "smoke: {} instances, topics seen {seen:?}",
        snap.instances.len()
    );
}

#[test]
fn every_frame_a_go_daemon_sends_re_encodes_byte_for_byte() {
    let Some(path) = socket() else { return };
    let mut s = LocalStream::connect(&path).unwrap();
    let reqs = [
        Request::hello("doze-control-smoke"),
        Request {
            id: 1,
            ..Request::op(Op::Status)
        },
        Request {
            id: 2,
            interval: "200ms".into(),
            ..Request::subscribe(&Subscription::to(&[
                Topic::Instances,
                Topic::Stats,
                Topic::Journal,
                Topic::Logs,
            ]))
        },
        Request {
            id: 3,
            ..Request::op(Op::Events)
        },
    ];
    for r in &reqs {
        s.write_all(&codec::encode(r).unwrap()).unwrap();
    }
    s.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut dec = codec::Decoder::new();
    let (mut lines, mut buf) = (0, [0u8; 65536]);
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        if let Ok(n) = s.read(&mut buf) {
            dec.push(&buf[..n]);
        }
        while let Some(line) = dec.next_line().unwrap() {
            let text = String::from_utf8(line.to_vec()).unwrap();
            let f: Frame = serde_json::from_str(&text).unwrap();
            assert_eq!(
                String::from_utf8(codec::encode(&f).unwrap()).unwrap(),
                format!("{text}\n")
            );
            lines += 1;
        }
    }
    for id in [2, 3] {
        s.write_all(&codec::encode(&Request::cancel(id)).unwrap())
            .unwrap();
    }
    assert!(lines >= 4, "{lines}");
    eprintln!("smoke: {lines} frames re-encoded byte for byte");
}
