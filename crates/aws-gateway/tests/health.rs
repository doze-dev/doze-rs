//! `GET /_doze/health`: unsigned, answered by the gateway from the facts it was handed, with its uptime added.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::net::SocketAddr;

use aws_gateway::{Config, Gateway};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn get(addr: SocketAddr, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8(raw).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    (
        head.split(' ').nth(1).unwrap().parse().unwrap(),
        body.to_owned(),
    )
}

#[tokio::test]
async fn health_answers_the_facts_it_was_given_and_its_uptime() {
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    let mut facts = serde_json::Map::new();
    facts.insert("version".into(), "1.2.3".into());
    config.health = Some(facts);
    let running = Gateway::new(config).bind().await.unwrap();
    let (status, body) = get(running.local_addr(), "/_doze/health").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["version"], "1.2.3");
    assert!(v["uptime_secs"].is_u64(), "{body}");
    running.shutdown().await;
}

#[tokio::test]
async fn without_facts_the_path_is_not_the_gateways() {
    let config = Config::new("127.0.0.1:0".parse().unwrap());
    let running = Gateway::new(config).bind().await.unwrap();
    let (status, _) = get(running.local_addr(), "/_doze/health").await;
    assert_ne!(status, 200);
    running.shutdown().await;
}
