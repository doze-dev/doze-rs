//! The Live tail over a real broker and a real recorder (CON-809, CON-810): rows arrive as HTML, newest first, once a second at most
//! and only when something was added; a reconnect from the cursor brings only what is newer; a folded repeat arrives again with
//! its count grown; the filters of the page are the filters of the stream.
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

use kafka_console::source::{Live, Source};
use kafka_wire::messages::*;
use support_live::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

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

async fn open(addr: std::net::SocketAddr, query: &str, last_event_id: Option<&str>) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let id = last_event_id.map_or(String::new(), |i| format!("Last-Event-ID: {i}\r\n"));
    s.write_all(
        format!("GET /classic/events?{query} HTTP/1.1\r\nHost: {addr}\r\n{id}\r\n").as_bytes(),
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
async fn rows_arrive_as_html_after_the_cursor_once_and_a_reconnect_brings_only_the_newer() {
    let (w, rec) = recorded("tail", false).await;
    w.create_topic("orders", 1, &[]).await;
    let c = follow(&w, &rec).await;
    let mut wire = Wire::connect(&w).await;
    wire.call(ApiKey::ApiVersions, 3, "first-app", &versions("kcat"))
        .await;
    let mut s = open(c.local_addr(), "tail=live&idle=0&since=0", None).await;
    read_until(&mut s, "event: ready", Duration::from_secs(2)).await;
    let text = read_until(&mut s, "first-app", Duration::from_secs(3)).await;
    assert!(text.contains("event: rows"), "{text}");
    assert!(
        text.contains(r#"<tr id=\"tape-r-"#) || text.contains(r#"<tr id="tape-r-"#),
        "{text}"
    );
    let id = text
        .lines()
        .find_map(|l| l.strip_prefix("id: "))
        .expect("an event id")
        .to_owned();
    drop(s);
    // Something newer happens while nobody is connected; the reconnect from `Last-Event-ID` brings that and not the old.
    wire.call(ApiKey::ApiVersions, 3, "second-app", &versions("kcat"))
        .await;
    let mut again = open(c.local_addr(), "tail=live&idle=0&since=0", Some(&id)).await;
    let text = read_until(&mut again, "second-app", Duration::from_secs(3)).await;
    assert!(!text.contains("first-app"), "{text}");
    c.shutdown().await;
    w.stop().await;
}

#[tokio::test]
async fn the_filters_of_the_page_are_the_filters_of_the_stream() {
    let (w, rec) = recorded("tailf", false).await;
    let c = follow(&w, &rec).await;
    let mut wire = Wire::connect(&w).await;
    let mut s = open(
        c.local_addr(),
        "tail=live&idle=0&client=wanted&since=0",
        None,
    )
    .await;
    read_until(&mut s, "event: ready", Duration::from_secs(2)).await;
    wire.call(ApiKey::ApiVersions, 3, "other", &versions("kcat"))
        .await;
    wire.call(ApiKey::ApiVersions, 3, "wanted", &versions("kcat"))
        .await;
    let text = read_until(&mut s, "wanted", Duration::from_secs(3)).await;
    assert!(!text.contains(">other<"), "{text}");
    c.shutdown().await;
    w.stop().await;
}
