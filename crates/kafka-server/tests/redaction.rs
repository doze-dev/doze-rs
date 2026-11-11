//! SEC-13: a planted password and the tokens that carry it appear nowhere a person could read them.
//!
//! This is a test binary of its own, with one test, so that the logging it installs is the process's and no
//! other test's callsites compete with it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use client::{ALICE, Client, SECRET, start_with};

/// The log writer of the test: everything the subscriber says, kept.
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

#[tokio::test]
async fn no_password_or_token_is_logged_at_any_level() {
    let logged = Logged::default();
    let sink = logged.clone();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .init();
    let server = start_with(true).await;

    // A failed login, a stranger's token, a good login and a second one on it.
    let mut bad = Client::connect(&server).await;
    bad.handshake("PLAIN").await.unwrap();
    let wrong = b"bob-planted-authzid\0alice\0wrong-password-planted-5521";
    let said = bad.token(wrong).await.unwrap();
    let said = format!("{said:?}");
    assert!(bad.is_closed().await);
    let mut stranger = Client::connect(&server).await;
    stranger
        .token(b"\0alice\0stranger-password-planted-0042")
        .await;
    let mut good = Client::connect(&server).await;
    good.log_in(ALICE).await;
    let again = format!("{:?}", good.token(ALICE).await.unwrap());
    drop((bad, stranger, good));
    // Let the connections write their own closing lines before the log is read.
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.shutdown().await;

    let text = String::from_utf8_lossy(&logged.0.lock().unwrap()).into_owned();
    assert!(
        text.contains("connection closed"),
        "the log was captured: {text:?}"
    );
    assert!(text.contains("logged in"), "{text}");
    // The log, and the texts the broker sends back (which a client may print): none holds a secret.
    let everything = format!("{text}\n{said}\n{again}");
    for planted in [
        "planted",
        SECRET,
        "alice-secret",
        "wrong-password",
        "stranger-password",
    ] {
        assert!(
            !everything.contains(planted),
            "{planted:?} is in:\n{everything}"
        );
    }
}
