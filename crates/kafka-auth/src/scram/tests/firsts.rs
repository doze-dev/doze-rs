//! The recorded refusals at client-first (`first-*`), and the extensions accepted there.

use super::*;

/// first-*: the seventeen refusals at client-first, and the two that are accepted.
#[test]
fn every_recorded_malformed_first_is_the_generic_refusal() {
    let tokens: [&[u8]; 17] = [
        b"",
        b"this is not a scram message",
        b"n=alice,r=NC.",
        b"n,,n=alice",
        b"n,,n=alice,r=",
        b"n,,r=NC.",
        b"n,,n=,r=NC.",
        b"n,,r=NC.,n=alice",
        b"p=tls-unique,,n=alice,r=NC.",
        b"y,,n=alice,r=NC.",
        b"x,,n=alice,r=NC.",
        b"n,,n=al=ice,r=NC.",
        b"n,,n=alic=65,r=NC.",
        b"n,,n=alice,r=NC.,,",
        b"n,,n=alice,r=nonce\xff",
        b"\0alice\0alice-secret",
        b"n,,n=alice,r=NC.,x",
    ];
    for kind in Kind::ALL {
        for token in tokens {
            let r = start(&users(), kind, token).unwrap_err();
            assert_eq!(
                r,
                rejected(kind, Why::BadFirst),
                "{:?}",
                String::from_utf8_lossy(token)
            );
        }
    }
}

/// first-mandatory-extension: accepted, then the proof fails because Kafka signs a message without it.
/// first-optional-extension: accepted and signed, the login succeeds.
#[test]
fn a_reserved_extension_is_not_signed_and_a_plain_extension_is() {
    for kind in Kind::ALL {
        let reserved = alice(kind).with_signed_prefix("m=ext,");
        assert_eq!(
            good(kind, &reserved).unwrap_err(),
            rejected(kind, Why::BadProof)
        );
        let reserved_unsigned = alice(kind).with_header("n,,m=ext,");
        assert!(good(kind, &reserved_unsigned).is_ok());
        let optional = alice(kind).with_extensions(",x=ext");
        assert!(good(kind, &optional).is_ok());
        let two = alice(kind).with_extensions(",x=ext,yy=a=b");
        assert!(good(kind, &two).is_ok());
        // A name sent twice counts once, with its last value: signing both is wrong, signing the last is right.
        let twice = alice(kind).with_extensions(",x=one,x=two");
        assert_eq!(
            good(kind, &twice).unwrap_err(),
            rejected(kind, Why::BadProof)
        );
        let users = users();
        let token = [alice(kind).first(), b",x=one,x=two".to_vec()].concat();
        let (sf, awaiting) = start(&users, kind, &token).unwrap();
        let signs_last = alice(kind).with_extensions(",x=two");
        assert!(
            awaiting
                .finish(&signs_last.last(&sf, None, "").message)
                .is_ok()
        );
    }
}

#[test]
fn delegation_token_logins_are_refused_as_an_unknown_user() {
    for value in ["true", "TRUE", "True"] {
        let c = alice(Kind::Sha256).with_extensions(&format!(",tokenauth={value}"));
        let r = start(&users(), Kind::Sha256, &c.first()).unwrap_err();
        assert_eq!(r, rejected(Kind::Sha256, Why::TokenAuth));
    }
    for value in ["false", "yes", "1", ""] {
        let c = alice(Kind::Sha256).with_extensions(&format!(",tokenauth={value}"));
        assert_eq!(
            start(&users(), Kind::Sha256, &c.first()).is_ok(),
            !value.is_empty()
        );
    }
}
