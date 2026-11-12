//! The TLS flags, in a build with the feature.

use super::*;

fn fixture(name: &str) -> String {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../kafka-server/tests/tls-fixtures");
    dir.join(name).display().to_string()
}

fn settings(flags: &[&str]) -> Result<crate::cli::Settings, String> {
    args(flags)
        .unwrap()
        .settings()
        .map_err(|e| format!("{e:#}"))
}

#[test]
fn a_certificate_and_a_key_turn_tls_on_and_none_leaves_it_off() {
    assert!(settings(&[]).unwrap().server.tls.is_none());
    let (cert, key) = (fixture("ec-chain.pem"), fixture("ec-key-pkcs8.pem"));
    let on = settings(&["--tls-cert", &cert, "--tls-key", &key]).unwrap();
    assert_eq!(
        on.server.tls.unwrap().cert_file().display().to_string(),
        cert
    );
}

#[test]
fn one_of_the_two_is_refused_in_the_go_editions_words() {
    for flags in [&["--tls-cert", "c.pem"][..], &["--tls-key", "k.pem"]] {
        let said = settings(flags).unwrap_err();
        assert_eq!(
            said, "config: TLS needs both --tls-cert and --tls-key, or neither",
            "{flags:?}"
        );
    }
}

#[test]
fn a_file_that_cannot_be_used_stops_the_start_naming_both_flags_and_no_key_bytes() {
    let said = settings(&[
        "--tls-cert",
        "nowhere-cert.pem",
        "--tls-key",
        "nowhere-key.pem",
    ])
    .unwrap_err();
    assert!(said.starts_with("config: cannot load the TLS certificate and key (--tls-cert nowhere-cert.pem, --tls-key nowhere-key.pem)"), "{said}");
    let (cert, key) = (fixture("ec-chain.pem"), fixture("rsa-key-pkcs8.pem"));
    let said = settings(&["--tls-cert", &cert, "--tls-key", &key]).unwrap_err();
    assert!(said.contains("does not match"), "{said}");
}
