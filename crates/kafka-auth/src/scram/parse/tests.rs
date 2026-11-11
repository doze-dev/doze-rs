//! The two parsers against the recorded `sasl-scram` shapes (docs/port/phase8/go-oracle-rust.md A2.2) and the
//! patterns of Kafka 4.3.1's `ScramMessages`.

use super::*;

fn accepted(token: &str) -> bool {
    first(token.as_bytes()).is_some()
}

#[test]
fn a_plain_client_first_is_read() {
    let m = first(b"n,,n=alice,r=nonce-of-x.").unwrap();
    assert_eq!(
        (m.authzid, m.saslname, m.nonce, m.extensions.len()),
        (None, "alice", "nonce-of-x.", 0)
    );
}

#[test]
fn an_authzid_is_kept_as_sent() {
    let m = first(b"n,a=alice,n=alice,r=NC.").unwrap();
    assert_eq!(m.authzid, Some("alice"));
    let m = first(b"n,a=al=2Cice,n=al=2Cice,r=NC.").unwrap();
    assert_eq!((m.authzid, m.saslname), (Some("al=2Cice"), "al=2Cice"));
}

/// first-empty .. first-plain-token: seventeen shapes Kafka's pattern refuses.
#[test]
fn the_recorded_malformed_firsts_are_refused() {
    let refused: [&[u8]; 20] = [
        b"",                                          // first-empty
        b"this is not a scram message",               // first-garbage
        b"n=alice,r=NC.",                             // first-no-gs2-header
        b"n,,n=alice",                                // first-no-nonce
        b"n,,n=alice,r=",                             // first-empty-nonce
        b"n,,r=NC.",                                  // first-no-user
        b"n,,n=,r=NC.",                               // first-empty-user
        b"n,,r=NC.,n=alice",                          // first-nonce-before-user
        b"p=tls-unique,,n=alice,r=NC.",               // first-channel-binding-required
        b"y,,n=alice,r=NC.",                          // first-channel-binding-y
        b"x,,n=alice,r=NC.",                          // first-unknown-gs2-flag
        b"n,,n=al=ice,r=NC.",                         // first-user-bad-escape
        b"n,,n=alic=65,r=NC.",                        // first-user-escaped-equals-sign-as-alice
        b"n,,n=alice,r=NC.,,",                        // first-nonce-with-comma
        b"n,,n=alice,r=nonce-of-first-not-utf8.\xff", // first-not-utf8
        b"\0alice\0alice-secret",                     // first-plain-token
        b"N,,n=alice,r=NC.",                          // the flag is lower case only
        b"n,,n=alice,r=NC.\0",                        // a NUL anywhere
        b"n,,n=alice,r=NC. x",                        // a space is not printable
        b"n,,n=alice,r=NC.\x7f",                      // nor is DEL
    ];
    for token in refused {
        assert_eq!(first(token), None, "{:?}", String::from_utf8_lossy(token));
    }
}

/// A line of the pattern at a time: what the flag and the name may be.
#[test]
fn the_header_the_name_and_the_nonce_are_as_strict_as_the_regex() {
    for ok in [
        "n,,n=al=2Cice,r=NC.", // first-user-escaped: an escape is accepted by the pattern (the user is unknown)
        "n,,n=al=3Dice,r=NC.",
        "n,,n= alice ,r=NC.", // a space is allowed in a name
        "n,,n=al\u{1}ice,r=NC.",
        "n,,n=a,r=!",
        "n,,n=alice,r=a=b", // '=' is allowed in a nonce
    ] {
        assert!(accepted(ok), "{ok}");
    }
    for bad in [
        "n,,n=al=2cice,r=NC.", // lower-case hex is not an escape
        "n,,n=al=,r=NC.",
        "n,,n=al=2,r=NC.",
        "n,,n=al=2",
        "n,a=,n=alice,r=NC.",       // an empty authzid
        "n,a=al=ice,n=alice,r=NC.", // authzid escapes are checked too
        "n,a=alice,m=,n=alice,r=NC.",
        "n,a=alice n=alice,r=NC.",
        "n,,n=alice;r=NC.",
        "n,,n=alice,r=NC.,",
        "n,,n=alice,r=NC.,x",
        "n,,n=alice,r=NC.,=x",
        "n,,n=alice,r=NC.,x=",
        "n,,n=alice,r=NC.,x1=y",
        "n,,n=alice,r=NC.,x=y,",
        "n,,n=alice,r=NC.\n",
    ] {
        assert!(!accepted(bad), "{bad:?}");
    }
}

/// first-mandatory-extension and first-optional-extension: both are accepted by the pattern.
#[test]
fn a_reserved_extension_and_extensions_after_the_nonce_are_read() {
    let m = first(b"n,,m=ext,n=alice,r=NC.").unwrap();
    assert_eq!((m.saslname, m.nonce), ("alice", "NC."));
    let m = first(b"n,a=alice,m=a=b,n=alice,r=NC.,x=ext,yy=a=b").unwrap();
    assert_eq!(m.extensions, [("x", "ext"), ("yy", "a=b")]);
    assert_eq!(m.authzid, Some("alice"));
}

#[test]
fn names_unescape_as_kafkas_formatter_does() {
    assert_eq!(unescape("alice"), "alice");
    assert_eq!(unescape("a=2Cb=3Dc"), "a,b=c");
    assert_eq!(unescape("=3D=2C"), "=,");
    assert_eq!(unescape("=3D2C"), "=2C");
}

fn done(token: &str) -> Option<Final<'_>> {
    last(token.as_bytes())
}

#[test]
fn a_client_final_is_read() {
    let m = done("c=biws,r=NC.SN,p=AAAA").unwrap();
    assert_eq!(
        (m.channel.as_slice(), m.nonce, m.proof),
        (&b"n,,"[..], "NC.SN", vec![0, 0, 0])
    );
}

/// final-empty .. final-trailing-attribute, plus the shapes of the pattern.
#[test]
fn the_recorded_malformed_finals_are_refused() {
    let nonce = "r=NC.SN";
    let proof = "p=AAAA";
    let refused = [
        String::new(),                                  // final-empty
        "this is not a scram message".to_owned(),       // final-garbage
        "c=biws,r=NC.SN".to_owned(),                    // final-no-proof
        "c=biws,r=NC.SN,p=!!!not-base64!!!".to_owned(), // final-proof-not-base64
        format!("c=biws,{proof}"),                      // final-no-nonce
        format!("{nonce},{proof}"),                     // final-no-channel-binding
        format!("c=!!!,{nonce},{proof}"),               // final-channel-binding-not-base64
        format!("c=biws,{nonce},{proof},x=ext"),        // final-trailing-attribute
        format!("{proof},c=biws,{nonce}"),              // out of order
        format!("c=biws,r=,{proof}"),                   // an empty nonce
        format!("c=biws,{nonce},p=AAA"),                // short padding
        format!("c=biws,{nonce},p=AA=A"),               // padding then data
        format!("c=biws,{nonce},p=AAAA=="),             // too much padding
        format!("c=biws,{nonce},p=AA==AAAA"),           // padding in the middle
        format!("c=biws,{nonce},p=AA AA"),              // white space
        format!("c=biws,{nonce},p=A-A_"),               // url-safe alphabet
        format!("c=biws,{nonce},x=ext"),                // an attribute for the proof
        format!("c=biws,{nonce},,{proof}"),             // an empty field
        format!("c=biws,{nonce},{proof}\0"),            // a NUL
        format!("c=biws,r=N C,{proof}"),                // a space in the nonce
        format!("c=biws,{nonce},x=,{proof}"),           // an empty extension value
    ];
    for token in refused {
        assert_eq!(done(&token), None, "{token:?}");
    }
    assert_eq!(last(b"c=biws,r=NC.SN,p=AAAA\xff"), None);
}

/// final-extension-before-proof: the pattern accepts it (Kafka then fails the proof, which does not cover it).
#[test]
fn extensions_before_the_proof_are_read_and_dropped() {
    assert!(done("c=biws,r=NC.SN,x=ext,p=AAAA").is_some());
    assert!(done("c=biws,r=NC.SN,x=ext,p=AAAA,p=BBBB").is_some());
    assert_eq!(
        done("c=biws,r=NC.SN,p=AAAA,p=BBBB").unwrap().proof,
        [4, 16, 65]
    );
}

/// The channel binding is only ever decoded: empty, odd, and not the header's are all read.
#[test]
fn the_channel_binding_is_decoded_and_nothing_more() {
    assert_eq!(done("c=,r=N,p=AAAA").unwrap().channel, b"");
    assert_eq!(done("c=eSws,r=N,p=AAAA").unwrap().channel, b"y,,");
    assert_eq!(
        done("c=bixhPWFsaWNlLA==,r=N,p=AAAA").unwrap().channel,
        b"n,a=alice,"
    );
    // Spare bits are ignored by Java's decoder, so by this one.
    assert_eq!(done("c=Zh==,r=N,p=Zm9=").unwrap().proof, b"fo");
}

/// final-proof-too-short and -empty reach the machine as a proof of the wrong length, not as a refusal here.
#[test]
fn a_proof_of_any_length_is_read() {
    assert_eq!(done("c=biws,r=N,p=").unwrap().proof, b"");
    assert_eq!(done("c=biws,r=N,p=AAAA").unwrap().proof.len(), 3);
    let long = format!("c=biws,r=N,p={}", "A".repeat(4096));
    assert_eq!(done(&long).unwrap().proof.len(), 3072);
}
