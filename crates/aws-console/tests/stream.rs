//! The Live tail and the limits, on a real listener: a tail opens at once with `ready`, says nothing while nothing is called, is
//! sent a call when one is recorded, carries on from a cursor, ends at once when the console stops, and has a cap. So have
//! connections, held against the ones in use and not the ones a browser left open.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aws_console::{Limits, Recorder, Running, Settings, start_with};
use support::{facts, sqs_call};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn console(limits: Limits) -> (Running, Arc<Recorder>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let recorder = Arc::new(Recorder::holding(64));
    let running = start_with(
        listener,
        facts(),
        Arc::clone(&recorder),
        Settings::default(),
        limits,
    )
    .unwrap();
    (running, recorder)
}

/// Opens a stream at `path` (with its query), as the console's reader does, with further header lines.
async fn open(addr: SocketAddr, path: &str, more: &str) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nX-Console: 1\r\nAccept: text/event-stream\r\n{more}\r\n"
    );
    s.write_all(req.as_bytes()).await.unwrap();
    s
}

/// Reads from the stream until the text so far contains `needle`; the text so far.
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
        assert!(n > 0, "the stream closed before {needle:?}; got {seen:?}");
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    seen
}

/// Reads the `ready` event (its id comes before its data).
async fn ready(s: &mut TcpStream) -> String {
    read_until(s, "\"reset\":", Duration::from_secs(2)).await
}

/// What arrives within `within`, if anything.
async fn read_some(s: &mut TcpStream, within: Duration) -> String {
    let mut buf = [0_u8; 4096];
    match tokio::time::timeout(within, s.read(&mut buf)).await {
        Ok(Ok(n)) => String::from_utf8_lossy(&buf[..n]).into_owned(),
        _ => String::new(),
    }
}

/// The `id:` of the last event in `text`.
fn last_id(text: &str) -> String {
    text.lines()
        .rev()
        .find_map(|l| l.strip_prefix("id:"))
        .map(|id| id.trim().to_owned())
        .unwrap_or_else(|| panic!("no id in {text:?}"))
}

#[tokio::test]
async fn a_tail_opens_with_ready_is_silent_while_nothing_is_called_and_sends_a_call_when_one_is() {
    let (c, recorder) = console(Limits::default()).await;
    let mut s = open(c.local_addr(), "/api/v1/tail/live", "").await;
    let seen = ready(&mut s).await + &read_some(&mut s, Duration::from_millis(100)).await;
    assert!(seen.starts_with("HTTP/1.1 200"), "{seen}");
    assert!(seen.contains("event: ready"), "{seen}");
    let lower = seen.to_ascii_lowercase();
    assert!(lower.contains("content-type: text/event-stream"), "{seen}");
    assert!(
        lower.contains("content-security-policy: default-src 'self'"),
        "{seen}"
    );
    assert!(lower.contains("x-content-type-options: nosniff"), "{seen}");
    assert!(seen.contains("\"reset\":false"), "{seen}");
    // Nothing is called: nothing is sent (the keep-alive comment is 15 s away).
    let quiet = read_some(&mut s, Duration::from_millis(600)).await;
    assert_eq!(quiet, "", "a quiet tail said something");
    // A call: one event, with the call in it.
    recorder.record(&sqs_call("SendMessage", 200, ""), 1_000);
    let got = read_until(&mut s, "\"action\":\"SendMessage\"", Duration::from_secs(2)).await;
    assert!(got.contains("event: calls"), "{got}");
    assert!(got.contains("\"service\":\"sqs\""), "{got}");
    assert!(got.contains("\"seq\":1"), "{got}");
    // A burst is a few events of many rows, not an event a call: the next event waits out the gap and carries all of them.
    for _ in 0..40 {
        recorder.record(&sqs_call("ReceiveMessage", 200, ""), 2_000);
    }
    let burst = read_until(&mut s, "\"seq\":41", Duration::from_secs(3)).await;
    assert_eq!(burst.matches("event: calls").count(), 1, "{burst}");
    assert_eq!(
        burst.matches("\"action\":\"ReceiveMessage\"").count(),
        40,
        "{burst}"
    );
    c.shutdown().await;
}

#[tokio::test]
async fn a_tail_carries_on_from_a_cursor_and_starts_over_after_another_run_s() {
    let (c, recorder) = console(Limits::default()).await;
    recorder.record(&sqs_call("CreateQueue", 200, ""), 1);
    let mut first = open(c.local_addr(), "/api/v1/tail/live", "").await;
    let cursor = last_id(&ready(&mut first).await);
    drop(first);
    // Two calls while nobody listened: a reader that comes back with its cursor is sent exactly those.
    recorder.record(&sqs_call("SendMessage", 200, ""), 2);
    recorder.record(&sqs_call("DeleteQueue", 200, ""), 3);
    let path = format!("/api/v1/tail/live?since={cursor}");
    let mut back = open(c.local_addr(), &path, "").await;
    let got = read_until(&mut back, "DeleteQueue", Duration::from_secs(2)).await;
    assert!(
        got.contains("SendMessage") && !got.contains("CreateQueue"),
        "{got}"
    );
    // `Last-Event-ID` (what a reader sends when its connection dropped) wins over `since`.
    let newest = last_id(&got);
    let mut again = open(
        c.local_addr(),
        &path,
        &format!("Last-Event-ID: {newest}\r\n"),
    )
    .await;
    let rest = ready(&mut again).await + &read_some(&mut again, Duration::from_millis(500)).await;
    assert!(!rest.contains("event: calls"), "{rest}");
    // The cursor of another run: told to start over, then sent this run's calls from the first held.
    let mut other = open(c.local_addr(), "/api/v1/tail/live?since=7.2", "").await;
    let got = read_until(&mut other, "DeleteQueue", Duration::from_secs(2)).await;
    assert!(got.contains("\"reset\":true"), "{got}");
    assert!(got.contains("CreateQueue"), "{got}");
    c.shutdown().await;
}

#[tokio::test]
async fn a_tail_ends_at_once_when_the_console_stops() {
    let (c, _recorder) = console(Limits::default()).await;
    let mut s = open(c.local_addr(), "/api/v1/tail/live", "").await;
    read_until(&mut s, "event: ready", Duration::from_secs(2)).await;
    let started = Instant::now();
    c.shutdown().await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    let mut rest = Vec::new();
    let ended = tokio::time::timeout(Duration::from_secs(2), s.read_to_end(&mut rest)).await;
    assert!(ended.is_ok(), "the stream did not end");
}

#[tokio::test]
async fn tails_have_a_cap_and_a_request_that_says_nothing_is_closed() {
    let limits = Limits {
        max_streams: 1,
        max_connections: 3,
        header_timeout: Duration::from_millis(300),
        ..Limits::default()
    };
    let (c, _recorder) = console(limits).await;
    let mut one = open(c.local_addr(), "/api/v1/tail/live", "").await;
    read_until(&mut one, "event: ready", Duration::from_secs(2)).await;
    // A second tail: 503 BUSY, in JSON, with Retry-After.
    let mut two = open(c.local_addr(), "/api/v1/tail/live", "Connection: close\r\n").await;
    let refused = read_until(&mut two, "\"code\":\"BUSY\"", Duration::from_secs(2)).await;
    assert!(refused.starts_with("HTTP/1.1 503"), "{refused}");
    assert!(
        refused.to_ascii_lowercase().contains("retry-after: 1"),
        "{refused}"
    );
    drop(two);
    // A connection that says nothing is closed when the time to send a head is up.
    let mut silent = TcpStream::connect(c.local_addr()).await.unwrap();
    let mut buf = Vec::new();
    let closed = tokio::time::timeout(Duration::from_secs(3), silent.read_to_end(&mut buf)).await;
    assert!(closed.is_ok(), "a silent connection was kept");
    c.shutdown().await;
}

#[tokio::test]
async fn the_tail_asks_for_the_proof_header_like_every_api_address() {
    let (c, _recorder) = console(Limits::default()).await;
    let mut s = TcpStream::connect(c.local_addr()).await.unwrap();
    let req = format!(
        "GET /api/v1/tail/live HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        c.local_addr()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).await.unwrap();
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    c.shutdown().await;
}

/// One `GET /healthz` on a connection that stays open (keep-alive); reads the whole answer, so the connection is then idle.
async fn visit_and_stay(addr: SocketAddr) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /healthz HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let got = read_until(&mut s, "ok", Duration::from_secs(2)).await;
    assert!(got.starts_with("HTTP/1.1 200"), "{got}");
    s
}

#[tokio::test]
async fn idle_keep_alive_connections_do_not_use_up_the_cap() {
    // What a browser leaves behind: Firefox and WebKit keep the connections of a page they have closed (and open speculative
    // ones) for as long as the server lets them, so a run of quick page loads fills the cap with connections that say nothing.
    let limits = Limits {
        max_connections: 3,
        ..Limits::default()
    };
    let (c, _recorder) = console(limits).await;
    let mut idle = Vec::new();
    for _ in 0..3 {
        idle.push(visit_and_stay(c.local_addr()).await);
    }
    // A fourth connection is served: the one that has been idle longest is given up for it.
    for round in 0..4 {
        let mut s = TcpStream::connect(c.local_addr()).await.unwrap();
        s.write_all(
            format!(
                "GET /healthz HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                c.local_addr()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut answer = String::new();
        tokio::time::timeout(Duration::from_secs(2), s.read_to_string(&mut answer))
            .await
            .expect("an answer")
            .unwrap();
        assert!(
            answer.starts_with("HTTP/1.1 200"),
            "round {round}: {answer}"
        );
    }
    // The connections that are being used are not given up: three open tails hold the cap, and a fourth connection is refused.
    drop(idle);
    let mut tails = Vec::new();
    for _ in 0..3 {
        let mut t = open(c.local_addr(), "/api/v1/tail/live", "").await;
        read_until(&mut t, "event: ready", Duration::from_secs(2)).await;
        tails.push(t);
    }
    let mut over = TcpStream::connect(c.local_addr()).await.unwrap();
    over.write_all(b"GET /healthz HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut answer = String::new();
    tokio::time::timeout(Duration::from_secs(2), over.read_to_string(&mut answer))
        .await
        .expect("an answer")
        .unwrap();
    // Over the cap, and every connection in use: answered 503 and closed, with the security headers every answer has.
    assert!(answer.starts_with("HTTP/1.1 503"), "{answer}");
    assert!(
        answer.contains("Content-Security-Policy: default-src 'self'"),
        "{answer}"
    );
    // And the tails are still open.
    for t in &mut tails {
        assert!(read_some(t, Duration::from_millis(100)).await.is_empty());
    }
    c.shutdown().await;
}
