//! The recorded refusals at client-final (`final-*`), and what is accepted there.

use super::*;

/// final-* that are 58: the generic refusal, with the reason in the log.
#[test]
fn every_recorded_malformed_final_is_the_generic_refusal() {
    // `{p}` stands for a correct proof of the exchange.
    let cases = [
        ("final-empty", "", Why::BadFinal),
        (
            "final-garbage",
            "this is not a scram message",
            Why::BadFinal,
        ),
        ("final-no-proof", "c=biws,r=NC.SN", Why::BadFinal),
        (
            "final-proof-not-base64",
            "c=biws,r=NC.SN,p=!!!not-base64!!!",
            Why::BadFinal,
        ),
        ("final-no-nonce", "c=biws,p={p}", Why::BadFinal),
        ("final-no-channel-binding", "r=NC.SN,p={p}", Why::BadFinal),
        (
            "final-channel-binding-not-base64",
            "c=!!!,r=NC.SN,p={p}",
            Why::BadFinal,
        ),
        (
            "final-wrong-nonce",
            "c=biws,r=NC.not-the-servers-half,p={p}",
            Why::WrongNonce,
        ),
        (
            "final-client-nonce-only",
            "c=biws,r=NC.,p={p}",
            Why::WrongNonce,
        ),
        (
            "final-nonce-with-suffix",
            "c=biws,r=NC.SNx,p={p}",
            Why::WrongNonce,
        ),
        (
            "final-trailing-attribute",
            "c=biws,r=NC.SN,p={p},x=ext",
            Why::BadFinal,
        ),
    ];
    let kind = Kind::Sha256;
    let c = alice(kind);
    for (name, template, why) in cases {
        let (sf, awaiting) = start(&users(), kind, &c.first()).unwrap();
        let good = String::from_utf8(c.last(&sf, None, "").message).unwrap();
        let proof = good.rsplit_once(",p=").unwrap().1;
        let message = template.replace("{p}", proof);
        let r = awaiting.finish(message.as_bytes()).unwrap_err();
        assert_eq!(r, rejected(kind, why), "{name}");
    }
    // A PLAIN token and a second client-first are final messages that do not match the pattern.
    for token in [&b"\0alice\0alice-secret"[..], b"n,,n=alice,r=NC."] {
        let (_, awaiting) = start(&users(), kind, &c.first()).unwrap();
        assert_eq!(
            awaiting.finish(token).unwrap_err(),
            rejected(kind, Why::BadFinal)
        );
    }
}
/// final-extension-before-proof: the harness signs the extension, Kafka's message has none, so the proof fails.
#[test]
fn an_extension_the_client_signed_before_the_proof_fails_the_proof() {
    for kind in Kind::ALL {
        let c = alice(kind);
        let r = exchange(kind, &c.first(), |sf| c.last(sf, None, ",x=ext").message);
        assert_eq!(r.unwrap_err(), rejected(kind, Why::BadProof));
    }
}

/// An extension the client did not sign is not covered by the proof either: the pattern lets it through.
#[test]
fn an_extension_before_the_proof_that_the_proof_does_not_cover_is_dropped() {
    let kind = Kind::Sha512;
    let c = alice(kind);
    let (sf, awaiting) = start(&users(), kind, &c.first()).unwrap();
    let plain = String::from_utf8(c.last(&sf, None, "").message).unwrap();
    let (head, proof) = plain.rsplit_once(",p=").unwrap();
    let with_extension = format!("{head},x=ext,p={proof}");
    assert!(awaiting.finish(with_extension.as_bytes()).is_ok());
}

/// final-wrong-channel-binding: the binding is never compared with the header; Kafka logs the client in.
#[test]
fn the_channel_binding_is_not_compared_with_the_header() {
    for kind in Kind::ALL {
        let c = alice(kind);
        for binding in ["eSws", "", "cD10bHMtdW5pcXVlLCw=", "AAAA"] {
            let r = exchange(kind, &c.first(), |sf| c.last(sf, Some(binding), "").message);
            assert!(r.is_ok(), "c={binding}");
        }
        // A binding that is not base64 is not a message at all.
        let r = exchange(kind, &c.first(), |sf| c.last(sf, Some("!!!"), "").message);
        assert_eq!(r.unwrap_err(), rejected(kind, Why::BadFinal));
    }
}

/// final-proof-too-short, final-proof-empty, proof-of-512-under-256: no answer, only a close.
#[test]
fn a_proof_that_is_not_a_hash_long_closes_without_an_answer() {
    let c = alice(Kind::Sha256);
    let thirty_one = format!("{}==", "A".repeat(42));
    for proof in ["AAAA", "", &thirty_one, &"A".repeat(44), &"A".repeat(4096)] {
        let r = exchange(Kind::Sha256, &c.first(), |_| {
            format!("c=biws,r=NC.SN,p={proof}").into_bytes()
        });
        assert_eq!(r.unwrap_err(), Refusal::ProofLength, "{}", proof.len());
    }
    // The proof of SHA-512 under SHA-256 (the server's salt is read from its own first message).
    let big = alice(Kind::Sha512);
    let r = exchange(Kind::Sha256, &c.first(), |sf| {
        big.last(sf, None, "").message
    });
    assert_eq!(r.unwrap_err(), Refusal::ProofLength);
    assert!(Refusal::ProofLength.is_silent());
    // And the proof of SHA-256 under SHA-512.
    let r = exchange(Kind::Sha512, &big.first(), |sf| {
        c.last(sf, None, "").message
    });
    assert_eq!(r.unwrap_err(), Refusal::ProofLength);
}

/// The nonce is judged before the proof's length, as Kafka does: a short proof with the wrong nonce is answered.
#[test]
fn the_nonce_is_checked_before_the_length_of_the_proof() {
    let c = alice(Kind::Sha256);
    let r = exchange(Kind::Sha256, &c.first(), |_| {
        b"c=biws,r=NC.wrong,p=AAAA".to_vec()
    });
    assert_eq!(r.unwrap_err(), rejected(Kind::Sha256, Why::WrongNonce));
    // And the pattern is judged before the nonce.
    let r = exchange(Kind::Sha256, &c.first(), |_| b"c=biws,r=NC.wrong".to_vec());
    assert_eq!(r.unwrap_err(), rejected(Kind::Sha256, Why::BadFinal));
}
