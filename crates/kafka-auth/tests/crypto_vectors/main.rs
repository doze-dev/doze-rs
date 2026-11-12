//! The in-tree PBKDF2 and base64 against vectors made by bun's `node:crypto` and `Buffer` (SEC-196), which share no code
//! with them. The vectors are in `vectors.txt`, made by `tools/gen-crypto-vectors.ts`; the lengths of the passwords lie
//! on both sides of the hash block sizes, where HMAC pads or hashes its key, and the iteration counts are 1, 2, 3 and
//! the 4096 and 5000 a login uses.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::num::NonZeroU32;

use kafka_auth::crypto::{base64, pbkdf2};

fn unhex(s: &str) -> Vec<u8> {
    if s == "-" {
        return Vec::new();
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn vectors() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/crypto_vectors/vectors.txt"
    ))
    .unwrap()
}

#[test]
fn pbkdf2_matches_node_crypto_for_both_hashes() {
    let mut seen = [0usize; 2];
    for line in vectors().lines().filter(|l| l.starts_with("P ")) {
        let f: Vec<&str> = line.split(' ').collect();
        let (password, salt, want) = (unhex(f[2]), unhex(f[3]), unhex(f[5]));
        let rounds = NonZeroU32::new(f[4].parse().unwrap()).unwrap();
        let (got, which) = match f[1] {
            "sha256" => (pbkdf2::sha256(&password, &salt, rounds).to_vec(), 0),
            "sha512" => (pbkdf2::sha512(&password, &salt, rounds).to_vec(), 1),
            other => panic!("unknown hash {other}"),
        };
        assert_eq!(got, want, "{line}");
        seen[which] += 1;
    }
    assert!(
        seen[0] >= 100 && seen[1] >= 100,
        "{seen:?}: the vectors are there"
    );
}

#[test]
fn base64_matches_node_buffer_both_ways_for_every_length() {
    let mut lengths = std::collections::BTreeSet::new();
    for line in vectors().lines().filter(|l| l.starts_with("B ")) {
        let f: Vec<&str> = line.split(' ').collect();
        let raw = unhex(f[1]);
        let text = if f[2] == "-" { "" } else { f[2] };
        assert_eq!(base64::encode(&raw), text, "{line}");
        assert_eq!(base64::decode(text.as_bytes()).unwrap(), raw, "{line}");
        assert_eq!(base64::decode_lax(text.as_bytes()).unwrap(), raw, "{line}");
        lengths.insert(raw.len() % 3);
    }
    assert_eq!(lengths.len(), 3, "all three paddings are covered");
}

#[test]
fn base64_refuses_what_is_not_the_canonical_form() {
    // Strict decoding refuses a length that is not a multiple of four, padding anywhere but the end, white space,
    // characters outside the alphabet, and bits set past the end of the data (a second encoding of the same bytes).
    for bad in [
        "A", "AA", "AAA", "AAAAA", "====", "A===", "AA=A", "A=AA", "AA==AAAA", " AAA", "AAA\n",
        "AA A", "AAA\0", "AB==", "AAB=", "@@@@", "AA==AA==",
    ] {
        assert!(base64::decode(bad.as_bytes()).is_err(), "{bad:?}");
    }
}
