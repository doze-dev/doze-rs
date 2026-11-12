//! The event stream of the two tails (CON-809, CON-810), on a real listener: it opens at once with `ready`, refuses anything that
//! is not a tail, ends at once when the console stops, and has a cap. No page opens it by itself (the page tests say so).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use kafka_console::serve::{Running, Settings, start};
use kafka_console::source::Source;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn console() -> Running {
    let settings = Settings {
        addr: "127.0.0.1:0".parse().unwrap(),
        prefix: "/".into(),
        allow_hosts: vec![],
    };
    start(Source::Fixture, settings).await.unwrap()
}

/// Opens a stream with `query` (the part after `/events?`).
async fn open(addr: SocketAddr, query: &str) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "GET /classic/events?{query} HTTP/1.1\r\nHost: {addr}\r\nAccept: text/event-stream\r\n\r\n"
    );
    s.write_all(req.as_bytes()).await.unwrap();
    s
}

/// Reads from the stream until the text so far contains `needle`; the text so far.
async fn read_until(s: &mut TcpStream, needle: &str, within: Duration) -> String {
    let mut seen = String::new();
    let deadline = Instant::now() + within;
    let mut buf = [0_u8; 1024];
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

#[tokio::test]
async fn a_tail_opens_at_once_with_ready_and_sends_nothing_while_nothing_is_added() {
    let c = console().await;
    let mut s = open(c.local_addr(), "tail=live&since=0").await;
    let seen = read_until(&mut s, "event: ready", Duration::from_secs(2)).await;
    assert!(seen.starts_with("HTTP/1.1 200"), "{seen}");
    let lower = seen.to_ascii_lowercase();
    assert!(lower.contains("content-type: text/event-stream"), "{seen}");
    assert!(lower.contains("cache-control: no-cache"), "{seen}");
    // The fixtures never change: three looks pass and no `rows` event is sent (the console does not talk to be talking).
    let mut more = [0_u8; 512];
    let quiet = tokio::time::timeout(Duration::from_millis(3200), s.read(&mut more)).await;
    assert!(quiet.is_err(), "an idle tail sent {:?}", &more[..8]);
    c.shutdown().await;
}

#[tokio::test]
async fn only_the_two_tails_can_be_followed() {
    let c = console().await;
    for q in ["watch=home-live", "tail=nonsense", "tail=", ""] {
        let mut s = open(c.local_addr(), q).await;
        let rest = read_until(
            &mut s,
            "tail must be live or messages",
            Duration::from_secs(2),
        )
        .await;
        assert!(rest.starts_with("HTTP/1.1 400"), "{q}: {rest}");
    }
    c.shutdown().await;
}

#[tokio::test]
async fn shutdown_while_ten_tails_are_open_completes_in_under_a_second() {
    let c = console().await;
    let addr = c.local_addr();
    let mut streams = Vec::new();
    for _ in 0..10 {
        let mut s = open(addr, "tail=live").await;
        read_until(&mut s, "event: ready", Duration::from_secs(2)).await;
        streams.push(s);
    }
    let started = Instant::now();
    c.shutdown().await;
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    for mut s in streams {
        let mut rest = Vec::new();
        let ended = tokio::time::timeout(Duration::from_secs(2), s.read_to_end(&mut rest)).await;
        assert!(ended.is_ok(), "a stream was left open after the shutdown");
    }
}

#[tokio::test]
async fn a_tail_that_goes_away_gives_its_place_back() {
    let c = console().await;
    let free = c.state().streams.available_permits();
    let mut a = open(c.local_addr(), "tail=live").await;
    read_until(&mut a, "event: ready", Duration::from_secs(2)).await;
    assert_eq!(c.state().streams.available_permits(), free - 1);
    drop(a);
    let deadline = Instant::now() + Duration::from_secs(6);
    while c.state().streams.available_permits() != free {
        assert!(
            Instant::now() < deadline,
            "the stream of a client that left was never dropped"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    c.shutdown().await;
}
