//! The registry on a real socket: answers in its own content type, refuses a body past the limit unread, and asks for
//! credentials with a challenge.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use std::sync::Arc;

use kafka_registry::api::{Api, Options};
use kafka_registry::server;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn exchange(addr: std::net::SocketAddr, head: &str, body: &[u8]) -> String {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(head.as_bytes()).await.unwrap();
    let _ = s.write_all(body).await;
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out).await;
    String::from_utf8_lossy(&out).into_owned()
}

async fn start(options: Options) -> server::Running {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    #[cfg(feature = "tls")]
    let tls = None;
    #[cfg(not(feature = "tls"))]
    let tls = ();
    server::start_on(
        listener,
        Arc::new(Api::new(common::registry(), options)),
        tls,
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_registration_over_tcp_is_answered_in_the_registrys_type() {
    let running = start(Options::default()).await;
    let body = common::schema_body(r#""string""#);
    let head = format!(
        "POST /subjects/s/versions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/vnd.schemaregistry.v1+json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let got = exchange(running.local_addr(), &head, body.as_bytes()).await;
    assert!(got.starts_with("HTTP/1.1 200"), "{got}");
    assert!(
        got.to_ascii_lowercase()
            .contains("content-type: application/vnd.schemaregistry.v1+json"),
        "{got}"
    );
    assert!(got.ends_with("{\"id\":1}\n"), "{got}");
    let big = vec![b'x'; 70 * 1024];
    let head = format!(
        "PUT /config HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        big.len()
    );
    let got = exchange(running.local_addr(), &head, &big).await;
    assert!(got.starts_with("HTTP/1.1 413"), "{got}");
    running.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_credentials_the_answer_is_a_challenge() {
    let running = start(Options {
        auth: Some(Arc::new(|_: &str, p: &str| p == "pw")),
        allowed_hosts: Vec::new(),
    })
    .await;
    let got = exchange(
        running.local_addr(),
        "GET /subjects HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        b"",
    )
    .await;
    assert!(got.starts_with("HTTP/1.1 401"), "{got}");
    assert!(
        got.to_ascii_lowercase()
            .contains("www-authenticate: basic realm=\"doze-kafka schema registry\""),
        "{got}"
    );
    running.shutdown().await;
}
