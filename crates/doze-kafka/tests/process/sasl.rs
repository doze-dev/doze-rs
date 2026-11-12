//! The binary with SASL flags: refused when they cannot work, and a client logs in when they do.

use super::*;

#[test]
fn flags_that_cannot_be_honoured_stop_the_start_and_say_why() {
    let data = scratch("refused");
    for (flags, why) in [
        (
            &["--tls-cert", "c.pem"][..],
            if cfg!(feature = "tls") { "TLS needs both --tls-cert and --tls-key" } else { "--features tls" },
        ),
        (
            &[
                "--sasl-mechanisms",
                "SCRAM-SHA-1",
                "--sasl-user",
                "u:planted-pw",
            ],
            "unknown SASL mechanism",
        ),
        (&["--sasl-user", "u:planted-pw"], "no mechanism is enabled"),
        (
            &["--sasl-mechanisms", "PLAIN"],
            "no credentials are configured",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
            .args(["--listen", "127.0.0.1:0", "--data-dir"])
            .arg(&data)
            .args(flags)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{flags:?}");
        assert!(said.contains(why), "{flags:?}: {said}");
        assert!(!said.contains("planted"), "{flags:?}: {said}");
    }
    let _ = std::fs::remove_dir_all(&data);
}

const PASSWORD: &str = "alice-planted-pw-8841";

/// A SaslHandshake and a SaslAuthenticate with `token`; the error code of the second.
fn log_in(s: &mut TcpStream, token: &[u8]) -> i16 {
    use kafka_protocol::messages::{
        SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest,
        SaslHandshakeResponse,
    };
    let mut hs = SaslHandshakeRequest::default();
    hs.mechanism = StrBytes::from_static_str("PLAIN");
    let said: SaslHandshakeResponse = call(s, ApiKey::SaslHandshake, 1, &hs);
    assert_eq!(said.error_code, 0);
    let mut auth = SaslAuthenticateRequest::default();
    auth.auth_bytes = Bytes::copy_from_slice(token);
    let said: SaslAuthenticateResponse = call(s, ApiKey::SaslAuthenticate, 2, &auth);
    said.error_code
}

#[test]
fn a_broker_started_with_sasl_logs_a_client_in_and_serves_it_and_prints_no_password() {
    let data = scratch("sasl");
    let broker = start_with(
        &data,
        &[
            "--sasl-mechanisms",
            "PLAIN",
            "--sasl-user",
            &format!("alice:{PASSWORD}"),
            "--log-level",
            "trace",
        ],
    );
    let mut wrong = broker.connect();
    assert_eq!(log_in(&mut wrong, b"\0alice\0nope"), 58);
    let mut right = broker.connect();
    let token = format!("\0alice\0{PASSWORD}");
    assert_eq!(log_in(&mut right, token.as_bytes()), 0);
    // Logged in, it is served: a topic is created.
    let mut topic = CreatableTopic::default();
    topic.name = TopicName(StrBytes::from_static_str("t"));
    topic.num_partitions = 1;
    topic.replication_factor = 1;
    let mut create = CreateTopicsRequest::default();
    create.topics.push(topic);
    create.timeout_ms = 5000;
    let made: CreateTopicsResponse = call(&mut right, ApiKey::CreateTopics, 7, &create);
    assert_eq!(made.topics[0].error_code, 0);
    drop((wrong, right));
    std::thread::sleep(Duration::from_millis(300));
    let log = broker.log.lock().unwrap().clone();
    assert!(log.contains("SASL is on"), "{log}");
    assert!(
        !log.contains("planted"),
        "the password is in the log:\n{log}"
    );
    drop(broker);
    let _ = std::fs::remove_dir_all(&data);
}

/// A SCRAM login over the wire: the handshake for `kind`, the two tokens; the error code of each.
fn scram_log_in(s: &mut TcpStream, kind: kafka_auth::scram::Kind, password: &str) -> (i16, i16) {
    use kafka_auth::scram::client::Client;
    use kafka_protocol::messages::{
        SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest,
        SaslHandshakeResponse,
    };
    let mut hs = SaslHandshakeRequest::default();
    hs.mechanism = StrBytes::from_string(kind.mechanism().name().to_owned());
    let said: SaslHandshakeResponse = call(s, ApiKey::SaslHandshake, 1, &hs);
    assert_eq!(said.error_code, 0);
    let me = Client::new(kind, "alice", password, "process-nonce-planted.");
    let mut auth = SaslAuthenticateRequest::default();
    auth.auth_bytes = Bytes::from(me.first());
    let first: SaslAuthenticateResponse = call(s, ApiKey::SaslAuthenticate, 2, &auth);
    if first.error_code != 0 {
        return (first.error_code, 0);
    }
    let last = me.last(&first.auth_bytes, None, "");
    auth.auth_bytes = Bytes::from(last.message);
    let done: SaslAuthenticateResponse = call(s, ApiKey::SaslAuthenticate, 2, &auth);
    if done.error_code == 0 {
        assert_eq!(done.auth_bytes.as_ref(), last.expected.as_slice());
    }
    (first.error_code, done.error_code)
}

#[test]
fn a_broker_started_with_scram_logs_a_client_in_with_either_hash_and_prints_no_password() {
    use kafka_auth::scram::Kind;
    let data = scratch("scram");
    let broker = start_with(
        &data,
        &[
            "--sasl-mechanisms",
            "PLAIN,SCRAM-SHA-512,SCRAM-SHA-256",
            "--sasl-user",
            &format!("alice:{PASSWORD}"),
            "--log-level",
            "trace",
        ],
    );
    for kind in [Kind::Sha256, Kind::Sha512] {
        let mut wrong = broker.connect();
        assert_eq!(scram_log_in(&mut wrong, kind, "wrong-planted-pw"), (0, 58));
        let mut right = broker.connect();
        assert_eq!(scram_log_in(&mut right, kind, PASSWORD), (0, 0));
        create(
            &mut right,
            if kind == Kind::Sha256 { "t256" } else { "t512" },
        );
    }
    std::thread::sleep(Duration::from_millis(300));
    let log = broker.log.lock().unwrap().clone();
    assert!(log.contains("SCRAM-SHA-512"), "{log}");
    assert!(!log.contains("planted"), "a secret is in the log:\n{log}");
    drop(broker);
    let _ = std::fs::remove_dir_all(&data);
}
