//! SEC-13 and SEC-49: a planted password and the tokens that carry it (PLAIN's, and SCRAM's nonces, proof and
//! keys) appear nowhere a person could read them.
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

use client::{ALICE, Client, SECRET, start_scram};
use kafka_auth::crypto::base64;
use kafka_auth::scram::Kind;
use kafka_auth::scram::client::Client as ScramClient;
use kafka_auth::scram::keys::ITERATIONS;

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
    let server = start_scram().await;

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
    // SCRAM: a wrong password, an unknown user, a proof of the wrong length, a good login.
    let mut hidden = scram_phases(&server).await;
    hidden.0.push(said.clone());
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
    let everything = format!("{text}\n{}\n{again}", hidden.0.join("\n"));
    let derived = hidden.1;
    for planted in [
        "planted",
        SECRET,
        "alice-secret",
        "wrong-password",
        "stranger-password",
    ]
    .into_iter()
    .chain(derived.iter().map(String::as_str))
    {
        assert!(
            !everything.contains(planted),
            "{planted:?} is in:\n{everything}"
        );
    }
}

/// What a SCRAM client could print of an exchange: the salt-derived keys, the proof, the signature and both
/// nonces, in hex and in base64.
fn secrets_of(kind: Kind, password: &str, challenge: &str, last: &str) -> Vec<String> {
    let mut fields = challenge.split(',');
    let nonce = fields.next().unwrap().strip_prefix("r=").unwrap();
    let salt = fields.next().unwrap().strip_prefix("s=").unwrap();
    let salt = base64::decode(salt.as_bytes()).unwrap();
    let salted = kind.salted_password(password.as_bytes(), &salt, ITERATIONS);
    let client_key = kind.hmac(&salted, &[b"Client Key"]);
    let stored = kind.hash(&client_key);
    let server_key = kind.hmac(&salted, &[b"Server Key"]);
    let proof = last.rsplit_once(",p=").unwrap().1.to_owned();
    let server_nonce = nonce.strip_prefix("client-nonce-planted-").unwrap();
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let mut out = vec![proof, server_nonce.split_once('.').unwrap().1.to_owned()];
    for key in [&salted, &client_key, &stored, &server_key] {
        out.push(base64::encode(key));
        out.push(hex(key));
    }
    out
}

/// Runs the SCRAM logins whose texts must stay out of the log. Returns what the broker answered to the
/// refused ones (to be searched too) and the derived secrets to search for.
async fn scram_phases(server: &kafka_server::Running) -> (Vec<String>, Vec<String>) {
    let (mut answers, mut derived) = (Vec::new(), Vec::new());
    for kind in [Kind::Sha256, Kind::Sha512] {
        let wrong = "wrong-scram-planted-8841";
        let me = ScramClient::new(kind, "alice", wrong, "client-nonce-planted-0099.");
        let mut c = Client::connect(server).await;
        let challenge = c.scram_first(kind, &me).await;
        let sf = String::from_utf8(challenge.auth_bytes.to_vec()).unwrap();
        let last = me.last(&challenge.auth_bytes, None, "");
        let last_text = String::from_utf8(last.message.clone()).unwrap();
        derived.extend(secrets_of(kind, wrong, &sf, &last_text));
        answers.push(format!("{:?}", c.token(&last.message).await.unwrap()));
        assert!(c.is_closed().await);

        // The good login of alice: its keys are derived from the real password.
        let me = ScramClient::new(kind, "alice", SECRET, "client-nonce-planted-0100.");
        let mut c = Client::connect(server).await;
        let challenge = c.scram_first(kind, &me).await;
        let sf = String::from_utf8(challenge.auth_bytes.to_vec()).unwrap();
        let last = me.last(&challenge.auth_bytes, None, "");
        let last_text = String::from_utf8(last.message.clone()).unwrap();
        derived.extend(secrets_of(kind, SECRET, &sf, &last_text));
        derived.push(String::from_utf8(last.expected.clone()).unwrap());
        let done = c.token(&last.message).await.unwrap();
        assert_eq!(done.error_code, 0);

        // An unknown user with a planted name, and a proof that is not a hash long.
        let mut c = Client::connect(server).await;
        let me = ScramClient::new(kind, "planted-user-3141", "x", "client-nonce-planted-0101.");
        answers.push(format!("{:?}", c.scram_first(kind, &me).await));
        let mut c = Client::connect(server).await;
        let me = ScramClient::new(kind, "alice", SECRET, "client-nonce-planted-0102.");
        let challenge = c.scram_first(kind, &me).await;
        let sf = String::from_utf8(challenge.auth_bytes.to_vec()).unwrap();
        let nonce = sf.split(',').next().unwrap().to_owned();
        derived.push(nonce.clone());
        let short = format!("c=biws,{nonce},p=AAAA");
        assert!(c.token(short.as_bytes()).await.is_none());
    }
    (answers, derived)
}
