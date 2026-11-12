//! The binary built with `--features tls` and given a certificate and key: a TLS client is served, a plaintext one is
//! not, a certificate that cannot be used stops the start (and never leaves a plaintext listener), and a stop with a
//! TLS connection open is clean.

use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use super::*;

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kafka-server/tests/tls-fixtures")
        .join(name)
        .display()
        .to_string()
}

type Secure = StreamOwned<ClientConnection, TcpStream>;

impl Broker {
    /// A TLS client that trusts the test root.
    fn connect_tls(&self) -> Secure {
        let mut roots = RootCertStore::empty();
        for der in CertificateDer::pem_file_iter(fixture("root.pem")).unwrap() {
            roots.add(der.unwrap()).unwrap();
        }
        let config = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let conn = ClientConnection::new(Arc::new(config), ServerName::try_from("localhost").unwrap()).unwrap();
        StreamOwned::new(conn, self.connect())
    }
}

fn flags<'a>(cert: &'a str, key: &'a str) -> [&'a str; 6] {
    ["--tls-cert", cert, "--tls-key", key, "--log-level", "trace"]
}

#[test]
fn a_broker_started_with_a_certificate_serves_tls_refuses_plaintext_and_prints_no_key() {
    let data = scratch("tls");
    let (cert, key) = (fixture("ec-chain.pem"), fixture("ec-key-pkcs8.pem"));
    let mut broker = start_with(&data, &flags(&cert, &key));
    let sent = batch();
    let mut s = broker.connect_tls();
    create(&mut s, "t");
    assert_eq!(produce(&mut s, "t", sent.clone()), 0);
    assert_eq!(&fetch(&mut s, "t")[8..], &sent[8..]);
    // Plaintext on the TLS port: closed, and not a byte of Kafka comes back.
    let mut plain = broker.connect();
    let request = framed(&request_payload(ApiKey::ApiVersions, 3, 1, Some("t"), &kafka_protocol::messages::ApiVersionsRequest::default()));
    plain.write_all(&request).unwrap();
    let mut said = Vec::new();
    let _ = plain.read_to_end(&mut said);
    assert!(said.len() <= 8, "{said:?}");
    // A stop with the TLS connection still open is clean.
    assert!(broker.terminate().success());
    let log = broker.log.lock().unwrap().clone();
    assert!(log.contains("TLS is on"), "{log}");
    assert!(log.contains("ec-chain.pem"), "the certificate is named: {log}");
    for line in std::fs::read_to_string(fixture("ec-key-pkcs8.pem")).unwrap().lines().filter(|l| !l.starts_with("-----")) {
        assert!(!log.contains(line), "key material in the log:\n{log}");
    }
    drop(s);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_certificate_that_cannot_be_used_stops_the_start_and_no_plaintext_listener_is_left() {
    let data = scratch("tls-bad");
    let missing = fixture("nowhere.pem");
    let (cert, key) = (fixture("ec-chain.pem"), fixture("rsa-key-pkcs8.pem"));
    for (c, k, why) in [
        (cert.as_str(), missing.as_str(), "nowhere.pem"),
        (cert.as_str(), key.as_str(), "does not match"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
            .args(["--listen", "127.0.0.1:0", "--data-dir"])
            .arg(&data)
            .args(["--tls-cert", c, "--tls-key", k])
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{why}");
        assert!(said.contains(why), "{said}");
        assert!(!said.contains("listening"), "it never got as far as a listener: {said}");
    }
    let _ = std::fs::remove_dir_all(&data);
}
