//! The binary with SASL flags: refused when they cannot work, and a client logs in when they do.

use super::*;

#[test]
fn flags_that_cannot_be_honoured_stop_the_start_and_say_why() {
    let data = scratch("refused");
    for (flags, why) in [
        (&["--tls-cert", "c.pem"][..], "TLS is not supported"),
        (
            &[
                "--sasl-mechanisms",
                "SCRAM-SHA-512",
                "--sasl-user",
                "u:planted-pw",
            ],
            "SCRAM arrives with slice 8c",
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
