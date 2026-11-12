//! SEC-157: the private key, and what a client sends in a broken handshake, appear nowhere a person could read
//! them: not in the log at any level, not in a `Debug` output, not in an error.
//!
//! A test binary of its own, with one test, so the logging it installs is the process's.
#![cfg(feature = "tls")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;
#[path = "support/tls_support.rs"]
mod support;

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kafka_server::TlsConfig;
use support::{Opts, connector, dial, fixture, handshake, start};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Clone, Default)]
struct Logged(Arc<Mutex<Vec<u8>>>);

impl io::Write for Logged {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The body lines of every key file: any of them in a text is key material in it.
fn key_lines() -> Vec<String> {
    [
        "ec-key-pkcs8.pem",
        "ec-key-sec1.pem",
        "rsa-key-pkcs8.pem",
        "rsa-key-pkcs1.pem",
    ]
    .iter()
    .flat_map(|f| {
        std::fs::read_to_string(fixture(f))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    })
    .filter(|l| !l.starts_with("-----") && l.len() > 20)
    .collect()
}

#[tokio::test]
async fn the_key_and_a_clients_broken_bytes_are_in_no_log_error_or_debug_output() {
    let logged = Logged::default();
    let sink = logged.clone();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .init();
    let mut said = Vec::new();
    // The errors and Debug of a load that fails and one that works.
    let loaded = TlsConfig::load(fixture("ec-chain.pem"), fixture("ec-key-pkcs8.pem")).unwrap();
    said.push(format!("{loaded:?}"));
    for (cert, key) in [
        ("ec-chain.pem", "rsa-key-pkcs8.pem"),
        ("ec-key-sec1.pem", "ec-key-sec1.pem"),
        ("ec-chain.pem", "ec-chain.pem"),
    ] {
        let e = TlsConfig::load(fixture(cert), fixture(key)).unwrap_err();
        said.push(format!("{e} {e:?}"));
    }
    let server = start(Opts::default()).await;
    // A good client, a plaintext one carrying a planted string, a broken record, an untrusting client.
    let mut good = dial(&server).await;
    assert!(good.versions().await.is_some());
    let mut http = TcpStream::connect(server.local_addr()).await.unwrap();
    http.write_all(b"GET /planted-client-bytes-5521 HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    let mut got = Vec::new();
    let _ = http.read_to_end(&mut got).await;
    let mut junk = TcpStream::connect(server.local_addr()).await.unwrap();
    junk.write_all(b"\x16\x03\x01\x00\x10planted-client-bytes-5521")
        .await
        .unwrap();
    let _ = junk.read_to_end(&mut got).await;
    assert!(
        handshake(&server, &connector("other-root.pem"))
            .await
            .is_err()
    );
    drop(good);
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.shutdown().await;

    let log = String::from_utf8_lossy(&logged.0.lock().unwrap()).into_owned();
    assert!(
        log.contains("connection closed"),
        "the log was captured: {log:?}"
    );
    assert!(
        log.contains("the TLS handshake failed: not a TLS handshake"),
        "{log}"
    );
    let everything = format!(
        "{log}\n{}\n{}",
        said.join("\n"),
        String::from_utf8_lossy(&got)
    );
    assert!(!everything.contains("planted-client-bytes"), "{everything}");
    for line in key_lines() {
        assert!(
            !everything.contains(&line),
            "key material in the output: {line}"
        );
    }
}
