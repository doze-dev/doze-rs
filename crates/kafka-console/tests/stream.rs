//! The event stream, on a real listener: the first event, a tick, a shutdown that does not wait for open streams, and a
//! task that is there only while somebody listens.
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

/// Opens a stream and reads until `needle` has arrived (or the time is up).
async fn open(addr: SocketAddr, watch: &str) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "GET /events?watch={watch} HTTP/1.1\r\nHost: {addr}\r\nAccept: text/event-stream\r\n\r\n"
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
async fn the_first_event_arrives_at_once_and_names_what_is_watched() {
    let c = console().await;
    let mut s = open(c.local_addr(), "home-instance,home-sentence").await;
    let seen = read_until(
        &mut s,
        "data: home-instance,home-sentence",
        Duration::from_secs(2),
    )
    .await;
    assert!(seen.starts_with("HTTP/1.1 200"), "{seen}");
    assert!(
        seen.to_ascii_lowercase()
            .contains("content-type: text/event-stream"),
        "{seen}"
    );
    assert!(seen.contains("event: tick"), "{seen}");
    assert!(
        seen.to_ascii_lowercase()
            .contains("cache-control: no-cache"),
        "{seen}"
    );
    c.shutdown().await;
}

#[tokio::test]
async fn a_tick_arrives_when_the_signal_is_bumped_and_by_itself_on_the_floor_timer() {
    let c = console().await;
    let mut s = open(c.local_addr(), "home-callout").await;
    read_until(&mut s, "data: home-callout\n\n", Duration::from_secs(2)).await;
    // A bump (the recorder's way, from slice 9e) reaches it at once.
    c.state().ticker.bump();
    let seen = read_until(&mut s, "data: home-callout\n\n", Duration::from_millis(500)).await;
    assert!(seen.contains("event: tick"));
    // And with nothing bumped, the 2 s floor timer ticks (a little over two seconds, never more than four).
    let started = Instant::now();
    read_until(&mut s, "data: home-callout\n\n", Duration::from_secs(4)).await;
    assert!(
        started.elapsed() >= Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
    c.shutdown().await;
}

#[tokio::test]
async fn shutdown_while_ten_streams_are_open_completes_in_under_a_second() {
    let c = console().await;
    let addr = c.local_addr();
    let mut streams = Vec::new();
    for _ in 0..10 {
        let mut s = open(addr, "home-instance").await;
        read_until(&mut s, "event: tick", Duration::from_secs(2)).await;
        streams.push(s);
    }
    assert_eq!(c.state().ticker.viewers(), 10);
    let started = Instant::now();
    c.shutdown().await;
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    // Every client sees its stream end, not hang.
    for mut s in streams {
        let mut rest = Vec::new();
        let ended = tokio::time::timeout(Duration::from_secs(2), s.read_to_end(&mut rest)).await;
        assert!(ended.is_ok(), "a stream was left open after the shutdown");
    }
}

#[tokio::test]
async fn the_task_exists_only_while_a_stream_is_connected() {
    let c = console().await;
    let ticker = c.state().ticker.clone();
    assert!(!ticker.running(), "nothing is connected: no task");
    let mut a = open(c.local_addr(), "home-instance").await;
    read_until(&mut a, "event: tick", Duration::from_secs(2)).await;
    let mut b = open(c.local_addr(), "home-instance").await;
    read_until(&mut b, "event: tick", Duration::from_secs(2)).await;
    assert!(ticker.running());
    assert_eq!(ticker.viewers(), 2);
    drop(a);
    wait_for(|| ticker.viewers() == 1).await;
    assert!(ticker.running(), "one stream is left");
    drop(b);
    wait_for(|| !ticker.running()).await;
    assert_eq!(ticker.viewers(), 0);
    c.shutdown().await;
}

/// A client that goes away is noticed on the next write (a tick or the keep-alive), so allow a floor period or two.
async fn wait_for(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(6);
    while !done() {
        assert!(
            Instant::now() < deadline,
            "the stream of a client that left was never dropped"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn an_unknown_fragment_is_refused_before_a_stream_is_opened() {
    let c = console().await;
    let mut s = open(c.local_addr(), "nonsense").await;
    let rest = read_until(&mut s, "is not a fragment", Duration::from_secs(2)).await;
    assert!(rest.starts_with("HTTP/1.1 400"), "{rest}");
    assert_eq!(c.state().ticker.viewers(), 0);
    c.shutdown().await;
}
