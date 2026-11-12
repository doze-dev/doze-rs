//! The keys of SCRAM against published and independently generated vectors.
//!
//! RFC 7677 section 3 gives the exchange for SHA-256 (user `user`, password `pencil`). It gives none for
//! SHA-512, so `tools/scram-vector.ts` computes both with Node's OpenSSL (`bun tools/scram-vector.ts`): its
//! SHA-256 output is the RFC's, byte for byte, which is what vouches for the SHA-512 output next to it.

use super::*;
use crate::crypto::base64;
use crate::crypto::tests::hex;

const SALT: &str = "W22ZaJ0SNY7soEsUEjb6gQ==";
const CLIENT_NONCE: &str = "rOprNGfwEbeRWgbNEkqO";
const SERVER_NONCE: &str = "%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0";

fn verifier(kind: Kind) -> Verifier {
    let salt = base64::decode(SALT.as_bytes()).unwrap();
    Verifier::derive(kind, b"pencil", &salt, ITERATIONS)
}

/// The three messages the AuthMessage of the RFC example is made of.
fn auth_message() -> String {
    let nonce = format!("{CLIENT_NONCE}{SERVER_NONCE}");
    format!("n=user,r={CLIENT_NONCE},r={nonce},s={SALT},i=4096,c=biws,r={nonce}")
}

#[test]
fn the_stored_and_server_keys_are_those_of_rfc_7677() {
    let v = verifier(Kind::Sha256);
    assert_eq!(
        hex(&v.stored_key),
        "586e5df283e6dceb5c3e791d8b8528ec191e664045ce971792e2e6b5bb13e2a6"
    );
    assert_eq!(
        hex(&v.server_key),
        "c1f3cbc1c13a9d35a14c0990eed97629ea225863e566a4314ab99f3f00e5d9d5"
    );
    let v = verifier(Kind::Sha512);
    assert_eq!(
        hex(&v.stored_key),
        "e8002e6f7d3ae446119b216933644dc2a2be7869eb918b8459b5e7d7d2ec12606aceef106825cd735170a675fd3611f684affad1dce3f43a0ee43bd590e1dbbe"
    );
    assert_eq!(
        hex(&v.server_key),
        "8d91db6230b5687874fe129bc7206e1858c3ae08e02934f57ac03b6b05a229c459d28ff46f5c9611e6c179256490215ec1ff759cb0df285db89af0f99e613aac"
    );
}

#[test]
fn the_proof_and_the_server_signature_are_those_of_the_rfc_and_of_openssl() {
    let message = auth_message();
    let cases = [
        (
            Kind::Sha256,
            "dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=",
            "6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=",
        ),
        (
            Kind::Sha512,
            "gMGXRcevScNtxZ6/8lQYpGtnsNAc3mGcmNomv+xnoOMw+3R2xNJdMNnzMlTN8PPC6wdp6dybEmDYXYTxwnYPJQ==",
            "ZQnYEgWQMFmmsM8aQMF0nDDCy/AgCzkwk8CmMZYcMg0vSVlKDanekLtifDSeVGT4+5ZxXnJq199RVG2rR7N7Zw==",
        ),
    ];
    for (kind, proof, signature) in cases {
        let v = verifier(kind);
        let proof = base64::decode(proof.as_bytes()).unwrap();
        assert!(v.proof_is_right(message.as_bytes(), &proof), "{kind:?}");
        assert_eq!(
            base64::encode(&v.server_signature(message.as_bytes())),
            signature,
            "{kind:?}"
        );
    }
}

#[test]
fn a_wrong_proof_or_a_proof_of_another_length_is_never_right() {
    let v = verifier(Kind::Sha256);
    let message = auth_message();
    let mut proof = base64::decode(b"dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=").unwrap();
    assert!(v.proof_is_right(message.as_bytes(), &proof));
    // One bit anywhere.
    for i in 0..proof.len() {
        proof[i] ^= 1;
        assert!(!v.proof_is_right(message.as_bytes(), &proof), "byte {i}");
        proof[i] ^= 1;
    }
    // Another message, another length, none, and the proof of the other hash.
    assert!(!v.proof_is_right(b"another message", &proof));
    assert!(!v.proof_is_right(message.as_bytes(), &proof[1..]));
    assert!(!v.proof_is_right(message.as_bytes(), &[]));
    assert!(!v.proof_is_right(message.as_bytes(), &[0; 64]));
    assert!(!v.proof_is_right(message.as_bytes(), &[0; 32]));
}

#[test]
fn the_kinds_map_to_mechanisms_and_sizes() {
    assert_eq!(Kind::of(Mechanism::Plain), None);
    for kind in Kind::ALL {
        assert_eq!(Kind::of(kind.mechanism()), Some(kind));
    }
    assert_eq!((Kind::Sha256.hash_len(), Kind::Sha512.hash_len()), (32, 64));
    assert_eq!(ITERATIONS.get(), 4096);
}

#[test]
fn a_verifier_prints_neither_a_key_nor_the_salt() {
    let v = verifier(Kind::Sha512);
    let shown = format!("{v:?} {v:#?}");
    assert!(
        shown.contains("Sha512") && shown.contains("4096"),
        "{shown}"
    );
    for secret in [
        hex(&v.stored_key),
        hex(&v.server_key),
        hex(&v.salt),
        SALT.to_owned(),
    ] {
        assert!(!shown.contains(&secret), "{shown}");
    }
}

/// A proof with bytes after the right ones is not the right proof (the comparison must not stop at the hash size).
#[test]
fn a_right_proof_with_bytes_added_is_wrong() {
    let v = verifier(Kind::Sha256);
    let mut proof = base64::decode(b"dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=").unwrap();
    proof.push(0);
    assert!(!v.proof_is_right(auth_message().as_bytes(), &proof));
}
