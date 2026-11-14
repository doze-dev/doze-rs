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

#[cfg(unix)]
#[test]
fn a_key_file_others_can_read_is_warned_about_and_a_private_one_is_not() {
    use std::os::unix::fs::PermissionsExt;
    // SEC-193: the fixture is copied, so that its mode in the repository is not what the test depends on.
    let dir = std::env::temp_dir().join(format!("doze-kafka-tlskey-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key = dir.join("key.pem");
    std::fs::copy(fixture("ec-key-pkcs8.pem"), &key).unwrap();
    let (cert, key_flag) = (fixture("ec-chain.pem"), key.display().to_string());
    let flags = ["--tls-cert", &cert, "--tls-key", &key_flag];
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
    let open = settings(&flags).unwrap();
    assert_eq!(
        open.warnings,
        [format!(
            "--tls-key {key_flag} is readable by group or others (mode 644); run chmod 600 on it"
        )]
    );
    assert!(open.server.tls.is_some(), "a warning, not a refusal");
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(settings(&flags).unwrap().warnings.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tls_alone_refuses_a_console_on_the_network_unless_unprotected() {
    // SEC-243: Kafka on every interface, TLS on, nothing else: the plain-HTTP console would hand out what TLS keeps.
    let (cert, key) = (fixture("ec-chain.pem"), fixture("ec-key-pkcs8.pem"));
    let flags = [
        "--tls-cert",
        &cert,
        "--tls-key",
        &key,
        "--listen",
        "0.0.0.0:9092",
        "--console-addr",
        "0.0.0.0:9093",
    ];
    let said = settings(&flags).unwrap_err();
    assert!(
        said.starts_with("config: the console has no login and is plain HTTP, and TLS is on: serving it on 0.0.0.0:9093 would let anyone who can reach that address read topics and messages sent unencrypted. Keep --console-addr on loopback"),
        "{said}"
    );
    let accepted: Vec<&str> = flags
        .iter()
        .copied()
        .chain(["--console-unprotected"])
        .collect();
    let set = settings(&accepted).unwrap();
    assert!(
        set.warnings
            .iter()
            .any(|w| w.contains("although TLS is on (--console-unprotected)")),
        "{:?}",
        set.warnings
    );
    assert!(!set.console.lab);
}
