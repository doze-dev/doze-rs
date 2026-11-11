//! Vectors: RFC 7914 section 11 for the first; the rest were made by OpenSSL (through Node's `crypto`), which
//! shares no code with this crate; and a differential test against the `pbkdf2` crate on arbitrary inputs.

use super::*;
use crate::crypto::tests::hex;

fn n(iterations: u32) -> NonZeroU32 {
    NonZeroU32::new(iterations).unwrap()
}

#[test]
fn sha_256_matches_rfc_7914() {
    // The first 32 of the 64 bytes of the RFC's PBKDF2-HMAC-SHA-256("passwd", "salt", 1, 64).
    assert_eq!(
        hex(&sha256(b"passwd", b"salt", n(1))),
        "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc"
    );
}

#[test]
fn sha_256_vectors() {
    let cases: [(&[u8], &[u8], u32, &str); 3] = [
        (
            b"password",
            b"salt",
            2,
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43",
        ),
        (
            b"password",
            b"salt",
            4096,
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a",
        ),
        (
            b"Password",
            b"NaCl",
            80000,
            "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56",
        ),
    ];
    for (password, salt, iterations, want) in cases {
        assert_eq!(hex(&sha256(password, salt, n(iterations))), want);
    }
}

#[test]
fn sha_512_vectors() {
    let cases: [(&[u8], &[u8], u32, &str); 3] = [
        (
            b"passwd",
            b"salt",
            1,
            "c74319d99499fc3e9013acff597c23c5baf0a0bec5634c46b8352b793e324723\
             d55caa76b2b25c43402dcfdc06cdcf66f95b7d0429420b39520006749c51a04e",
        ),
        (
            b"password",
            b"salt",
            2,
            "e1d9c16aa681708a45f5c7c4e215ceb66e011a2e9f0040713f18aefdb866d53c\
             f76cab2868a39b9f7840edce4fef5a82be67335c77a6068e04112754f27ccf4e",
        ),
        (
            b"password",
            b"salt",
            4096,
            "d197b1b33db0143e018b12f3d1d1479e6cdebdcc97c5c0f87f6902e072f457b5\
             143f30602641b3d55cd335988cb36b84376060ecd532e039b742a239434af2d5",
        ),
    ];
    for (password, salt, iterations, want) in cases {
        assert_eq!(hex(&sha512(password, salt, n(iterations))), want);
    }
}

/// The salted password of the RFC 7677 example (SCRAM-SHA-256, user "user", password "pencil", `i=4096`).
#[test]
fn the_scram_example_salt_and_password() {
    let salt = crate::crypto::base64::decode(b"W22ZaJ0SNY7soEsUEjb6gQ==").unwrap();
    assert_eq!(
        hex(&sha256(b"pencil", &salt, n(4096))),
        "c4a49510323ab4f952cac1fa99441939e78ea74d6be81ddf7096e87513dc615d"
    );
    assert_eq!(
        hex(&sha512(b"pencil", &salt, n(4096))),
        "f16efe1be67f1d09502ebd5ed9262fddffba5a377ab4f0b687e5ed5ba0f50686\
         b8a4ae166476da8ab3b951d2fa9238b63998f45461bc33a464814949cec9631d"
    );
}

#[test]
fn it_agrees_with_the_pbkdf2_crate_on_awkward_inputs() {
    let passwords: [&[u8]; 4] = [b"", b"p", &[0xff; 130], b"pass word\0with nul"];
    let salts: [&[u8]; 3] = [b"", b"salt", &[0; 40]];
    for password in passwords {
        for salt in salts {
            for iterations in [1, 2, 3, 50] {
                let mut want256 = [0u8; 32];
                pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password, salt, iterations, &mut want256);
                assert_eq!(sha256(password, salt, n(iterations)), want256);
                let mut want512 = [0u8; 64];
                pbkdf2::pbkdf2_hmac::<sha2::Sha512>(password, salt, iterations, &mut want512);
                assert_eq!(sha512(password, salt, n(iterations)), want512);
            }
        }
    }
}
