use super::*;

/// Lower-case hex of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn sha_256_and_512_match_fips_180_for_abc() {
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&sha512(b"abc")),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
         2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
}

/// RFC 4231 test cases 1, 2 and 6 (a key longer than the block, which HMAC hashes first).
#[test]
fn hmac_matches_rfc_4231() {
    let long = [0xaa; 131];
    let cases: [(&[u8], &[u8], &str, &str); 3] = [
        (
            &[0x0b; 20],
            b"Hi There",
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            "87aa7cdea5ef619d4ff0b4241a1d6cb02379f4e2ce4ec2787ad0b30545e17cde\
             daa833b7d6b8a702038b274eaea3f4e4be9d914eeb61f1702e696c203a126854",
        ),
        (
            b"Jefe",
            b"what do ya want for nothing?",
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea250554\
             9758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737",
        ),
        (
            &long,
            b"Test Using Larger Than Block-Size Key - Hash Key First",
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
            "80b24263c7c1a3ebb71493c1dd7be8b49b46d1f41b4aeec1121b013783f8f352\
             6b56d037e05f2598bd0fd2215d6a1e5295e64f73f63f0aec8b915a985d786598",
        ),
    ];
    for (key, data, want256, want512) in cases {
        assert_eq!(hex(&hmac_sha256(key, &[data])), want256);
        assert_eq!(hex(&hmac_sha512(key, &[data])), want512);
    }
}

#[test]
fn hmac_of_parts_is_hmac_of_their_concatenation() {
    let whole = hmac_sha256(b"key", &[b"hello world"]);
    assert_eq!(hmac_sha256(b"key", &[b"hello", b" ", b"world"]), whole);
    assert_eq!(hmac_sha256(b"key", &[b"", b"hello world", b""]), whole);
}

#[test]
fn hmac_takes_a_key_of_any_length_without_failing_closed() {
    // The all-zero answer is the fail-closed fallback; no key length may reach it.
    for len in [0, 1, 63, 64, 65, 127, 128, 129, 1000] {
        let key = vec![7u8; len];
        assert_ne!(hmac_sha256(&key, &[b"x"]), [0; 32], "key of {len}");
        assert_ne!(hmac_sha512(&key, &[b"x"]), [0; 64], "key of {len}");
    }
}

#[test]
fn equality_is_exact_and_length_sensitive() {
    assert!(ct_eq(b"secret", b"secret"));
    assert!(ct_eq(b"", b""));
    assert!(!ct_eq(b"secret", b"secreT"));
    assert!(!ct_eq(b"secret", b"secre"));
    assert!(!ct_eq(b"secre", b"secret"));
    assert!(!ct_eq(b"", b"x"));
}
