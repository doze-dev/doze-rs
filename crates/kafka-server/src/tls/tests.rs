#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tls-fixtures").join(name)
}

fn scratch(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kafka-server-tls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    std::fs::write(&file, text).unwrap();
    file
}

fn load(cert: &str, key: &str) -> Result<TlsConfig, TlsError> {
    TlsConfig::load(fixture(cert), fixture(key))
}

#[test]
fn a_chain_loads_with_a_key_in_any_of_the_three_formats() {
    for (cert, key) in [
        ("ec-chain.pem", "ec-key-pkcs8.pem"),
        ("ec-chain.pem", "ec-key-sec1.pem"),
        ("rsa-chain.pem", "rsa-key-pkcs8.pem"),
        ("rsa-chain.pem", "rsa-key-pkcs1.pem"),
        ("ec-leaf-only.pem", "ec-key-sec1.pem"),
    ] {
        let loaded = load(cert, key).unwrap_or_else(|e| panic!("{cert} with {key}: {e}"));
        assert_eq!(loaded.cert_file(), fixture(cert));
    }
}

#[test]
fn a_missing_file_is_named() {
    let missing = fixture("nowhere.pem");
    let said = TlsConfig::load(&missing, fixture("ec-key-sec1.pem")).unwrap_err().to_string();
    assert!(said.contains("nowhere.pem"), "{said}");
    let said = TlsConfig::load(fixture("ec-chain.pem"), &missing).unwrap_err().to_string();
    assert!(said.contains("nowhere.pem"), "{said}");
}

#[test]
fn an_empty_file_or_the_wrong_kind_of_file_says_what_is_missing_and_where() {
    let empty = scratch("empty.pem", "");
    let said = TlsConfig::load(&empty, fixture("ec-key-sec1.pem")).unwrap_err();
    assert!(matches!(said, TlsError::NoCertificate { .. }), "{said}");
    assert!(said.to_string().contains("empty.pem"));
    let said = TlsConfig::load(fixture("ec-chain.pem"), &empty).unwrap_err();
    assert!(matches!(said, TlsError::NoKey { .. }), "{said}");
    // The files the wrong way round: a key where the certificate goes, and a certificate where the key goes.
    let said = load("ec-key-sec1.pem", "ec-key-sec1.pem").unwrap_err();
    assert!(matches!(said, TlsError::NoCertificate { .. }), "{said}");
    let said = load("ec-chain.pem", "ec-chain.pem").unwrap_err();
    assert!(matches!(said, TlsError::NoKey { .. }), "{said}");
}

#[test]
fn a_key_that_belongs_to_another_certificate_is_a_mismatch_naming_both_files() {
    let said = load("ec-chain.pem", "rsa-key-pkcs8.pem").unwrap_err();
    assert!(matches!(said, TlsError::Mismatch { .. }), "{said}");
    let text = said.to_string();
    assert!(text.contains("ec-chain.pem") && text.contains("rsa-key-pkcs8.pem"), "{text}");
    let said = load("rsa-chain.pem", "ec-key-sec1.pem").unwrap_err();
    assert!(matches!(said, TlsError::Mismatch { .. }), "{said}");
}

#[test]
fn an_encrypted_key_is_recognised_and_garbage_is_not_echoed() {
    let encrypted = scratch(
        "encrypted.pem",
        "-----BEGIN ENCRYPTED PRIVATE KEY-----\nMIHsMFcGCSqGSIb3DQEFDTBKMCkGCSqGSIb3DQEFDDAcBAhplanted\n-----END ENCRYPTED PRIVATE KEY-----\n",
    );
    let said = TlsConfig::load(fixture("ec-chain.pem"), &encrypted).unwrap_err();
    assert!(matches!(said, TlsError::EncryptedKey { .. }), "{said}");
    // A section that opens and never closes, and one with a body that is not base64: the parser's errors quote lines.
    for (name, body) in [
        ("open.pem", "-----BEGIN PRIVATE KEY-----\nSECRETKEYMATERIALplanted7731\n"),
        ("base64.pem", "-----BEGIN PRIVATE KEY-----\n!!planted-secret-9921!!\n-----END PRIVATE KEY-----\n"),
        ("junk-key.pem", "-----BEGIN PRIVATE KEY-----\nAAAAAAAAplantedAAAA\n-----END PRIVATE KEY-----\n"),
    ] {
        let file = scratch(name, body);
        let said = TlsConfig::load(fixture("ec-chain.pem"), &file).unwrap_err();
        let text = format!("{said} {said:?}");
        assert!(!text.contains("planted"), "{name}: {text}");
        assert!(text.contains(name), "{name}: {text}");
    }
}

#[test]
fn the_key_is_in_no_debug_output_and_the_files_are_named_instead() {
    let loaded = load("ec-chain.pem", "ec-key-pkcs8.pem").unwrap();
    let config = crate::Config { tls: Some(loaded), ..crate::Config::default() };
    let shown = format!("{config:?}");
    assert!(shown.contains("ec-key-pkcs8.pem"), "{shown}");
    let key_body: String = std::fs::read_to_string(fixture("ec-key-pkcs8.pem"))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    assert!(!shown.contains(&key_body[..24]), "{shown}");
}

#[test]
fn what_went_wrong_in_a_handshake_is_named_without_quoting_the_peer() {
    let eof = io::Error::from(io::ErrorKind::UnexpectedEof);
    assert_eq!(kind(&eof), "closed during the handshake");
    let other = io::Error::other("planted client text");
    assert_eq!(kind(&other), "an i/o error");
    let alert = io::Error::new(
        io::ErrorKind::InvalidData,
        rustls::Error::AlertReceived(rustls::AlertDescription::BadCertificate),
    );
    assert_eq!(kind(&alert), "the client sent an alert");
}
