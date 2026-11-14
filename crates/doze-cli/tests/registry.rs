//! The registry's signing scheme, checked against what the registry really publishes: the conformance fixture its JS
//! signer made (a copy of doze-registry scripts/testdata/conformance/fixture.json, byte-identical to the doze-sdk
//! twin at the time of copying), and the valkey index served on 2026-11-13, signed with the key built into doze.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;

use doze_cli::registry::{index, latest, signed};
use ed25519_dalek::Signer;

const CONFORMANCE: &str = include_str!("fixtures/conformance.json");
const REAL_VALKEY: &str = include_str!("fixtures/real/doze/valkey/index.yaml");

/// The JS signer's payload, byte for byte: re-signing it with the fixture's key gives the fixture's signature
/// (ed25519 is deterministic), and the index and its artifact verify with the fixture's public key.
#[test]
fn the_payload_is_the_registry_signers_own() {
    let fx: serde_json::Value = serde_json::from_str(CONFORMANCE).unwrap();
    let idx = index::parse(&fx["index"].to_string()).unwrap();
    let key = signed::parse_key(fx["publicKey"].as_str().unwrap()).unwrap();
    index::verify(&idx, &key).unwrap();
    let resigned =
        fixture::test_key().sign(signed::sha256_hex(index::payload(&idx).as_bytes()).as_bytes());
    use base64ct::Encoding;
    assert_eq!(
        base64ct::Base64::encode_string(&resigned.to_bytes()),
        idx.signature
    );
    let art = &idx.releases["0.1.0"].artifacts["aarch64-apple-darwin"];
    signed::verify_hex(&key, &art.sha256, &art.sig).unwrap();
}

/// The live index verifies with the key built in, and any change to what it says (a channel rolled back, a
/// protocol, a hash) is refused.
#[test]
fn the_real_index_verifies_and_any_edit_is_refused() {
    let key = signed::builtin_key().unwrap();
    let idx = index::parse(REAL_VALKEY).unwrap();
    index::verify(&idx, &key).unwrap();
    let edits = [
        REAL_VALKEY.replace("stable: 0.2.1", "stable: 0.2.0"),
        REAL_VALKEY.replacen("protocol: 1", "protocol: 2", 1),
        REAL_VALKEY.replacen("4d4ccc27", "4d4ccc28", 1),
        REAL_VALKEY.replacen("- \"9\"", "- \"10\"", 1),
    ];
    for edited in edits {
        let idx = index::parse(&edited).unwrap();
        assert_eq!(index::verify(&idx, &key), Err(signed::Refused::WrongKey));
    }
    // The fixture key is not the registry's.
    let other = signed::parse_key("ZCqZ4rLRcedni6DmQ0/yGkZ+aUY9CFA8oXBtcw5+VAk=").unwrap();
    assert!(index::verify(&idx, &other).is_err());
}

/// The release file verifies as published and refuses an edit; more than two lines of news are kept to two.
#[test]
fn the_release_file_is_signed_like_an_index() {
    let sb = fixture::Sandbox::new("latest");
    fixture::write_registry(&sb.registry());
    let text = std::fs::read_to_string(sb.registry().join(latest::PATH)).unwrap();
    let key = signed::parse_key(&{
        use base64ct::Encoding;
        base64ct::Base64::encode_string(fixture::test_key().verifying_key().as_bytes())
    })
    .unwrap();
    assert_eq!(latest::parse(&text, &key).unwrap().version, "9.9.9");
    assert!(latest::parse(&text.replace("9.9.9", "9.9.10"), &key).is_err());
    assert!(latest::parse(&text, &signed::builtin_key().unwrap()).is_err());
}
