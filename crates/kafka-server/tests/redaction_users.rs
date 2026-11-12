//! SEC-178: the password behind a user made over the API, the salted password the client sends for it, and the keys the
//! broker derives from that appear nowhere a person could read them, at any log level.
//!
//! A test binary of its own, with one test, so that the logging it installs is the process's.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;
#[path = "support/users_server.rs"]
mod server;

use std::io;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use client::Client;
use kafka_auth::crypto::base64;
use kafka_auth::scram::{Kind, Verifier};
use kafka_storage::sim::SimFs;
use server::{admin, alter, deletion, refused, start, upsert};

const PASSWORD: &str = "dave-planted-password-7781";

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

/// `bytes` as a log line could show them: base64, hex, and the decimal list of a `Debug` of a byte slice.
fn forms(bytes: &[u8]) -> [String; 3] {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let list = bytes
        .iter()
        .take(8)
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    [base64::encode(bytes), hex, list]
}

#[tokio::test]
async fn a_password_a_salted_password_and_the_keys_made_from_it_are_in_no_log() {
    let logged = Logged::default();
    let sink = logged.clone();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .init();
    let server = start(&SimFs::new()).await;
    let mut root = admin(&server).await;
    // What the client sends for each mechanism, and what the broker derives from it.
    let mut secrets = vec![PASSWORD.to_owned()];
    for kind in [Kind::Sha256, Kind::Sha512] {
        let salt = b"salt-dave";
        let iterations = NonZeroU32::new(4096).unwrap();
        let salted = kind.salted_password(PASSWORD.as_bytes(), salt, iterations);
        let v = Verifier::from_salted_password(kind, salt, iterations, &salted);
        let (stored, server_key) = v.keys();
        for bytes in [&salted[..], stored, server_key] {
            secrets.extend(forms(bytes));
        }
        let made = alter(&mut root, vec![], vec![upsert("dave", kind, PASSWORD)]).await;
        assert_eq!(made, vec![("dave".to_owned(), 0)]);
        Client::connect(&server)
            .await
            .scram_log_in(kind, "dave", PASSWORD)
            .await;
        refused(&server, kind, "dave", "wrong-password-planted-0042").await;
    }
    // A request that is refused, and a deletion: the same bytes cross the wire.
    let denied = alter(
        &mut root,
        vec![],
        vec![upsert("admin", Kind::Sha256, PASSWORD)],
    )
    .await;
    assert_eq!(denied, vec![("admin".to_owned(), 93)]);
    let gone = alter(&mut root, vec![deletion("dave", 1)], vec![]).await;
    assert_eq!(gone, vec![("dave".to_owned(), 0)]);
    drop(root);
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.shutdown().await;

    let text = String::from_utf8_lossy(&logged.0.lock().unwrap()).into_owned();
    assert!(text.contains("connection closed"), "{text:?}");
    assert!(text.contains("logged in"), "{text}");
    assert!(text.len() > 500, "the log was captured: {text:?}");
    for secret in secrets {
        assert!(!text.contains(&secret), "{secret:?} is in the log:\n{text}");
    }
    assert!(!text.contains("planted"), "{text}");
}
