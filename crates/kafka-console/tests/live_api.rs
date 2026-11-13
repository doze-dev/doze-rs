//! The Live endpoints of the React console (slice R3): the snapshot, a request in full with the rules behind a refusal, the
//! clients, the first request, the docks, and the JSON tail over a real broker and recorder: rows after the cursor, a reconnect
//! from `Last-Event-ID` bringing only the newer, a cursor of another life answered with `reset`, the pace clamped.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_live;
mod support_write;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use kafka_console::app::console;
use kafka_console::base::Base;
use kafka_console::limits::Limits;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_wire::messages::*;
use support::{HOST, send};
use support_live::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

fn fixtures() -> axum::Router {
    console(State::console(
        Source::Fixture,
        &Base::new("/"),
        vec![],
        CancellationToken::new(),
        &Limits::default(),
    ))
}

async fn get(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .unwrap();
    let (res, body) = send(app, req).await;
    (
        res.status(),
        serde_json::from_str(&body).unwrap_or_default(),
    )
}

#[tokio::test]
async fn the_live_api_says_the_tape_the_requests_the_clients_and_the_docks() {
    let app = fixtures();
    let (s, live) = get(&app, "/api/v1/live").await;
    assert_eq!(s, StatusCode::OK);
    let rows = live["live"]["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    assert!(rows[0]["id"].as_u64().is_some());
    assert!(live["tail_from"].as_str().unwrap().contains('.'));
    assert!(live["words"]["empty"].as_str().unwrap().contains("kcat -P"));
    assert_eq!(live["live"]["window"]["capacity"], 2000);
    let (_, filtered) = get(
        &app,
        "/api/v1/live?refused=1&client=orders&client=admin-cli",
    )
    .await;
    assert_eq!(
        filtered["words"]["phrases"],
        serde_json::json!(["app is orders or admin-cli", "it was refused"])
    );
    // Every refusal on the tape opens the Sheet its kind calls for, with the rules behind it.
    let mut kinds = std::collections::BTreeSet::new();
    for r in rows {
        let id = r["id"].as_u64().unwrap();
        let (s, d) = get(&app, &format!("/api/v1/live/request/{id}")).await;
        assert_eq!(s, StatusCode::OK, "{id}");
        let d = &d["found"];
        let kind = d["kind"].as_str().unwrap().to_owned();
        if kind != "detail" {
            assert!(!d["rules"].as_array().unwrap().is_empty(), "{id}: {d}");
        }
        kinds.insert(kind);
    }
    let (_, fenced) = get(&app, "/api/v1/live/request/99").await;
    assert_eq!(fenced["found"]["kind"], "fenced");
    kinds.insert("fenced".to_owned());
    assert!(
        kinds.contains("refused") && kinds.contains("too_large") && kinds.contains("detail"),
        "{kinds:?}"
    );
    let (s, gone) = get(&app, "/api/v1/live/request/987654321").await;
    assert_eq!(s, StatusCode::OK);
    assert!(gone["found"].is_null());
    assert!(gone["gone"].as_str().unwrap().contains("no longer on the wire"));
    let (s, junk) = get(&app, "/api/v1/live/request/not-a-number").await;
    assert_eq!(s, StatusCode::OK);
    assert!(junk["found"].is_null());
    let (_, clients) = get(&app, "/api/v1/live/clients").await;
    assert!(!clients["rows"].as_array().unwrap().is_empty());
    let (_, first) = get(&app, "/api/v1/live/first").await;
    assert!(first["first"]["client"].as_str().is_some());
    let (_, dock) = get(&app, "/api/v1/live/topic-wire?topic=orders").await;
    assert_eq!(dock["topic"], "orders");
    let (_, group) = get(
        &app,
        "/api/v1/live/group-wire?group=fulfilment&kind=heartbeats",
    )
    .await;
    assert_eq!(group["group"], "fulfilment");
    assert_eq!(group["query"]["kind"], "heartbeats");
}

async fn follow(
    w: &support_write::World,
    rec: &std::sync::Arc<kafka_console::recorder::Recorder>,
) -> kafka_console::serve::Running {
    let live =
        Live::new(w.backend.clone(), w.facts.clone()).with_recorder(std::sync::Arc::clone(rec));
    kafka_console::serve::start(
        Source::Live(Box::new(live)),
        kafka_console::serve::Settings {
            addr: "127.0.0.1:0".parse().unwrap(),
            prefix: "/".into(),
            allow_hosts: vec![],
        },
    )
    .await
    .unwrap()
}

async fn open(
    addr: std::net::SocketAddr,
    query: &str,
    last_event_id: Option<&str>,
    proof: bool,
) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let id = last_event_id.map_or(String::new(), |i| format!("Last-Event-ID: {i}\r\n"));
    let proof = if proof {
        "X-Console: 1\r\nSec-Fetch-Site: same-origin\r\n"
    } else {
        ""
    };
    s.write_all(
        format!("GET /api/v1/tail/live?{query} HTTP/1.1\r\nHost: {addr}\r\n{proof}{id}\r\n")
            .as_bytes(),
    )
    .await
    .unwrap();
    s
}

async fn read_until(s: &mut TcpStream, needle: &str, within: Duration) -> String {
    let mut seen = String::new();
    let deadline = Instant::now() + within;
    let mut buf = [0_u8; 4096];
    while !seen.contains(needle) {
        let left = deadline.saturating_duration_since(Instant::now());
        let n = tokio::time::timeout(left, s.read(&mut buf))
            .await
            .unwrap_or_else(|_| panic!("no {needle:?} within {within:?}; got {seen:?}"))
            .unwrap();
        assert!(n > 0, "closed before {needle:?}; got {seen:?}");
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    seen
}

#[tokio::test]
async fn the_json_tail_sends_rows_after_the_cursor_once_and_a_reconnect_brings_only_the_newer() {
    let (w, rec) = recorded("rtail", false).await;
    let c = follow(&w, &rec).await;
    let addr = c.local_addr();
    // Without the proof header: refused, like every API request.
    let mut bare = open(addr, "idle=0", None, false).await;
    read_until(&mut bare, "403", Duration::from_secs(2)).await;
    let mut wire = Wire::connect(&w).await;
    let mut s = open(addr, "idle=0&pace=0", None, true).await;
    let ready = read_until(&mut s, "event: ready", Duration::from_secs(2)).await;
    assert!(ready.contains("text/event-stream"), "{ready}");
    let ready = read_until(&mut s, "\"pace\":1", Duration::from_secs(2)).await;
    assert!(ready.contains("\"reset\":false"), "{ready}");
    wire.call(ApiKey::ApiVersions, 3, "first-app", &versions("kcat"))
        .await;
    let text = read_until(&mut s, "first-app", Duration::from_secs(3)).await;
    assert!(text.contains("event: rows"), "{text}");
    assert!(text.contains("\"rows\":[{\"id\":"), "{text}");
    let id = text
        .lines()
        .filter_map(|l| l.strip_prefix("id: "))
        .last()
        .expect("an event id")
        .to_owned();
    drop(s);
    wire.call(ApiKey::ApiVersions, 3, "second-app", &versions("kcat"))
        .await;
    let mut again = open(addr, "idle=0", Some(&id), true).await;
    let text = read_until(&mut again, "second-app", Duration::from_secs(5)).await;
    assert!(!text.contains("first-app"), "{text}");
    drop(again);
    // A cursor of another life (the broker restarted): `reset`, and the rows of this life.
    let mut other = open(addr, "idle=0", Some("1.999999"), true).await;
    let text = read_until(&mut other, "second-app", Duration::from_secs(5)).await;
    assert!(text.contains("\"reset\":true"), "{text}");
    drop(other);
    // The filters of the page are the filters of the stream; the pace is clamped to 30 s at most.
    let mut f = open(addr, "idle=0&client=wanted&pace=9999", None, true).await;
    let text = read_until(&mut f, "\"pace\":30", Duration::from_secs(2)).await;
    assert!(text.contains("event: ready"), "{text}");
    drop(f);
    c.shutdown().await;
    w.stop().await;
}
