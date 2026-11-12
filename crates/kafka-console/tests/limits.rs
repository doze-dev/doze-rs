//! What one console connection may cost (audit A12), over real sockets: a head that never completes, a connection that
//! says nothing, a body that never arrives, a cap on connections and a cap on event streams, and the slots coming back.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use kafka_console::serve::{Running, Settings, start_with};
use kafka_console::source::Source;
use kafka_console::{Limits, MAX_CONNECTIONS, MAX_STREAMS};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn console(limits: Limits) -> Running {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let settings = Settings {
        addr: listener.local_addr().unwrap(),
        prefix: "/".into(),
        allow_hosts: vec![],
    };
    start_with(listener, Source::Fixture, settings, limits).unwrap()
}

fn small() -> Limits {
    Limits {
        max_connections: 6,
        max_streams: 3,
        header_timeout: Duration::from_millis(300),
        request_timeout: Duration::from_millis(300),
    }
}

/// Reads until the peer closes (or `within` passes: then the connection was still open). The text read, and whether it ended.
async fn read_to_close(s: &mut TcpStream, within: Duration) -> (String, bool) {
    let mut out = Vec::new();
    let mut buf = [0_u8; 2048];
    let end = Instant::now() + within;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, s.read(&mut buf)).await {
            Err(_) => return (String::from_utf8_lossy(&out).into_owned(), false),
            Ok(Ok(0) | Err(_)) => return (String::from_utf8_lossy(&out).into_owned(), true),
            Ok(Ok(n)) => out.extend_from_slice(&buf[..n]),
        }
    }
}

async fn read_until(s: &mut TcpStream, needle: &str, within: Duration) -> String {
    let mut seen = String::new();
    let end = Instant::now() + within;
    let mut buf = [0_u8; 2048];
    while !seen.contains(needle) {
        let left = end.saturating_duration_since(Instant::now());
        let n = tokio::time::timeout(left, s.read(&mut buf))
            .await
            .unwrap_or_else(|_| panic!("no {needle:?} within {within:?}; got {seen:?}"))
            .unwrap();
        assert!(n > 0, "closed before {needle:?}; got {seen:?}");
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    seen
}

async fn dial(addr: SocketAddr) -> TcpStream {
    TcpStream::connect(addr).await.unwrap()
}

async fn open_stream(addr: SocketAddr) -> TcpStream {
    let mut s = dial(addr).await;
    s.write_all(format!("GET /events?watch=home-live HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    s
}

#[test]
fn the_documented_numbers_are_the_defaults() {
    let d = Limits::default();
    assert_eq!((d.max_connections, d.max_streams), (256, 64));
    assert_eq!((MAX_CONNECTIONS, MAX_STREAMS), (256, 64));
    assert_eq!(d.header_timeout, Duration::from_secs(10));
    assert_eq!(d.request_timeout, Duration::from_secs(30));
}

#[tokio::test]
async fn a_partial_head_a_silent_connection_and_an_idle_one_are_closed_after_the_timeout() {
    let c = console(small()).await;
    let addr = c.local_addr();
    // Half a head.
    let mut partial = dial(addr).await;
    partial
        .write_all(format!("GET / HTTP/1.1\r\nHost: {addr}\r\nX-Slow: ").as_bytes())
        .await
        .unwrap();
    let started = Instant::now();
    let (_, closed) = read_to_close(&mut partial, Duration::from_secs(3)).await;
    assert!(closed, "a partial head was still open after three seconds");
    assert!(
        started.elapsed() < Duration::from_millis(1800),
        "{:?}",
        started.elapsed()
    );
    // Nothing at all.
    let mut silent = dial(addr).await;
    let started = Instant::now();
    let (_, closed) = read_to_close(&mut silent, Duration::from_secs(3)).await;
    assert!(
        closed,
        "a silent connection was still open after three seconds"
    );
    assert!(
        started.elapsed() < Duration::from_millis(1800),
        "{:?}",
        started.elapsed()
    );
    // A request that was answered and then nothing (keep-alive).
    let mut idle = dial(addr).await;
    idle.write_all(format!("GET /healthz HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let seen = read_until(&mut idle, "ok\n", Duration::from_secs(2)).await;
    assert!(seen.starts_with("HTTP/1.1 200"), "{seen}");
    let (_, closed) = read_to_close(&mut idle, Duration::from_secs(3)).await;
    assert!(
        closed,
        "an idle keep-alive connection was still open after three seconds"
    );
    c.shutdown().await;
}

#[tokio::test]
async fn a_body_that_stops_arriving_is_answered_408_and_the_connection_closed() {
    let c = console(small()).await;
    let addr = c.local_addr();
    let mut s = dial(addr).await;
    s.write_all(
        format!(
            "POST /connect/advertised/check HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 100\r\n\r\nback="
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let started = Instant::now();
    let (seen, closed) = read_to_close(&mut s, Duration::from_secs(3)).await;
    assert!(seen.starts_with("HTTP/1.1 408"), "{seen}");
    assert!(
        seen.to_ascii_lowercase()
            .contains("content-security-policy: default-src 'self'"),
        "the 408 has the security headers: {seen}"
    );
    assert!(closed, "{seen}");
    assert!(
        started.elapsed() < Duration::from_millis(1800),
        "{:?}",
        started.elapsed()
    );
    c.shutdown().await;
}

#[tokio::test]
async fn the_connection_over_the_cap_is_told_to_come_back_and_closing_connections_frees_the_slots()
{
    let limits = Limits {
        header_timeout: Duration::from_secs(20),
        request_timeout: Duration::from_secs(20),
        ..small()
    };
    let c = console(limits).await;
    let addr = c.local_addr();
    // Six connections that say nothing hold the six slots.
    let mut held = Vec::new();
    for _ in 0..6 {
        held.push(dial(addr).await);
    }
    // Give the accept loop its turn, then the seventh is over the cap.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut seventh = dial(addr).await;
    seventh
        .write_all(format!("GET /healthz HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let (seen, closed) = read_to_close(&mut seventh, Duration::from_secs(3)).await;
    let lower = seen.to_ascii_lowercase();
    assert!(seen.starts_with("HTTP/1.1 503"), "{seen}");
    assert!(lower.contains("retry-after: 1"), "{seen}");
    assert!(
        lower.contains("content-security-policy: default-src 'self'"),
        "{seen}"
    );
    assert!(lower.contains("x-frame-options: deny"), "{seen}");
    assert!(lower.contains("x-content-type-options: nosniff"), "{seen}");
    assert!(lower.contains("referrer-policy: same-origin"), "{seen}");
    assert!(closed, "{seen}");
    // Close two: two more are served.
    held.truncate(4);
    tokio::time::sleep(Duration::from_millis(200)).await;
    for _ in 0..2 {
        let mut again = dial(addr).await;
        again
            .write_all(
                format!("GET /healthz HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let (seen, _) = read_to_close(&mut again, Duration::from_secs(3)).await;
        assert!(
            seen.starts_with("HTTP/1.1 200"),
            "a freed slot was not given back: {seen}"
        );
    }
    drop(held);
    c.shutdown().await;
}

#[tokio::test]
async fn the_stream_over_the_cap_is_refused_and_a_stream_that_ends_gives_its_slot_back() {
    let limits = Limits {
        header_timeout: Duration::from_secs(20),
        request_timeout: Duration::from_secs(20),
        max_connections: 32,
        ..small()
    };
    let c = console(limits).await;
    let addr = c.local_addr();
    let mut streams = Vec::new();
    for _ in 0..3 {
        let mut s = open_stream(addr).await;
        read_until(&mut s, "event: tick", Duration::from_secs(2)).await;
        streams.push(s);
    }
    let mut fourth = open_stream(addr).await;
    let (seen, _) = read_to_close(&mut fourth, Duration::from_secs(3)).await;
    let lower = seen.to_ascii_lowercase();
    assert!(seen.starts_with("HTTP/1.1 503"), "{seen}");
    assert!(lower.contains("retry-after: 1"), "{seen}");
    assert!(
        lower.contains("content-security-policy: default-src 'self'"),
        "{seen}"
    );
    assert!(lower.contains("event streams open"), "{seen}");
    // Pages are not streams: they are still served.
    let mut page = dial(addr).await;
    page.write_all(
        format!("GET /healthz HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap();
    assert!(
        read_to_close(&mut page, Duration::from_secs(2))
            .await
            .0
            .starts_with("HTTP/1.1 200")
    );
    // One stream goes; the console notices on its next write, and the slot is free.
    streams.pop();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        c.state().ticker.bump();
        let mut retry = open_stream(addr).await;
        let (seen, _) = read_to_close(&mut retry, Duration::from_millis(400)).await;
        if seen.starts_with("HTTP/1.1 200") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the slot of a stream that ended was never given back: {seen}"
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    drop(streams);
    c.shutdown().await;
}
