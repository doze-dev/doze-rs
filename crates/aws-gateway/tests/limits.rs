//! The gateway's hold on its connections: a client that sends nothing is closed, and past the cap a connection waits its turn.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::time::Duration;

use aws_gateway::{Config, Gateway, Limits};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

fn gateway(limits: Limits) -> Gateway {
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    config.health = Some(serde_json::Map::new());
    Gateway::new(config).with_limits(limits)
}

const HEALTH: &[u8] = b"GET /_doze/health HTTP/1.1\r\nHost: x\r\n\r\n";

/// Reads one answer's head, or `None` if the connection was closed first.
async fn head(stream: &mut TcpStream) -> Option<String> {
    let mut got = Vec::new();
    let mut buf = [0u8; 1024];
    while !got.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        got.extend_from_slice(&buf[..n]);
    }
    Some(String::from_utf8_lossy(&got).into_owned())
}

#[tokio::test]
async fn a_connection_that_sends_nothing_is_closed_and_so_is_one_left_idle() {
    let limits = Limits {
        header_timeout: Duration::from_millis(200),
        ..Limits::default()
    };
    let running = gateway(limits).bind().await.unwrap();
    let mut silent = TcpStream::connect(running.local_addr()).await.unwrap();
    let mut buf = [0u8; 64];
    let closed = timeout(Duration::from_secs(5), silent.read(&mut buf)).await;
    assert!(matches!(closed, Ok(Ok(0) | Err(_))), "{closed:?}");
    // Kept alive after an answer, then idle: closed the same way.
    let mut idle = TcpStream::connect(running.local_addr()).await.unwrap();
    idle.write_all(HEALTH).await.unwrap();
    assert!(head(&mut idle).await.unwrap().starts_with("HTTP/1.1 200"));
    let mut rest = Vec::new();
    let closed = timeout(Duration::from_secs(5), idle.read_to_end(&mut rest)).await;
    assert!(closed.is_ok(), "an idle kept-alive connection is closed");
    running.shutdown().await;
}

#[tokio::test]
async fn past_the_cap_a_connection_is_served_when_another_closes() {
    let limits = Limits {
        max_connections: 1,
        ..Limits::default()
    };
    let running = gateway(limits).bind().await.unwrap();
    let mut first = TcpStream::connect(running.local_addr()).await.unwrap();
    first.write_all(HEALTH).await.unwrap();
    assert!(head(&mut first).await.is_some());
    let mut second = TcpStream::connect(running.local_addr()).await.unwrap();
    second.write_all(HEALTH).await.unwrap();
    let waited = timeout(Duration::from_millis(300), head(&mut second)).await;
    assert!(waited.is_err(), "the second waits while the first is open");
    drop(first);
    let served = timeout(Duration::from_secs(5), head(&mut second)).await;
    assert!(served.unwrap().unwrap().starts_with("HTTP/1.1 200"));
    running.shutdown().await;
}
