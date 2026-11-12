//! Every recorded step of `sasl-scram` (docs/port/phase8/go-oracle-rust.md A2.2), driven by an independent
//! client, and the RFC 7677 exchange through the machine.

use std::num::NonZeroU32;

use client::Client;

use super::*;
use crate::credentials::Credentials;

mod finals;
mod firsts;

const SN: &str = "SN";
const NC: &str = "NC.";

fn users() -> Credentials {
    let pairs = [
        ("admin", "admin-secret"),
        ("alice", "alice-secret"),
        ("bob", "bob-secret"),
    ];
    Credentials::new(pairs.map(|(u, p)| (u.to_owned(), p.to_owned())))
        .unwrap()
        .with_scram(&Kind::ALL)
        .unwrap()
}

fn start(users: &Credentials, kind: Kind, token: &[u8]) -> Result<(Vec<u8>, Awaiting), Refusal> {
    first(kind, token, SN, |u| users.verifier(kind, u))
}

fn alice(kind: Kind) -> Client {
    Client::new(kind, "alice", "alice-secret", NC)
}

fn rejected(kind: Kind, why: Why) -> Refusal {
    Refusal::Rejected {
        mechanism: kind.mechanism(),
        why,
    }
}

/// A whole exchange: the first message, and the final one built by `finish` from the server-first message.
fn exchange(
    kind: Kind,
    first_token: &[u8],
    finish: impl FnOnce(&[u8]) -> Vec<u8>,
) -> Result<(Vec<u8>, Principal), Refusal> {
    let (server_first, awaiting) = start(&users(), kind, first_token)?;
    awaiting.finish(&finish(&server_first))
}

fn good(kind: Kind, c: &Client) -> Result<(Vec<u8>, Principal), Refusal> {
    exchange(kind, &c.first(), |sf| c.last(sf, None, "").message)
}

#[test]
fn rfc_7677_through_the_machine() {
    let salt = base64::decode(b"W22ZaJ0SNY7soEsUEjb6gQ==").unwrap();
    let server_nonce = "%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0";
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
        let v = Arc::new(Verifier::derive(kind, b"pencil", &salt, keys::ITERATIONS));
        let token = b"n,,n=user,r=rOprNGfwEbeRWgbNEkqO";
        let (server_first, awaiting) = first(kind, token, server_nonce, |_| Some(v)).unwrap();
        assert_eq!(
            String::from_utf8(server_first).unwrap(),
            "r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096"
        );
        let last = format!("c=biws,r=rOprNGfwEbeRWgbNEkqO{server_nonce},p={proof}");
        let (reply, who) = awaiting.finish(last.as_bytes()).unwrap();
        assert_eq!(String::from_utf8(reply).unwrap(), format!("v={signature}"));
        assert_eq!(who.as_str(), "User:user");
    }
}

/// good-256, good-512: the server-first message has exactly `r=,s=,i=` and the signature is the client's.
#[test]
fn a_right_password_logs_in_with_both_hashes() {
    for kind in Kind::ALL {
        let c = alice(kind);
        let (server_first, awaiting) = start(&users(), kind, &c.first()).unwrap();
        let text = String::from_utf8(server_first.clone()).unwrap();
        let mut f = text.split(',');
        assert_eq!(f.next(), Some("r=NC.SN"));
        let salt = f.next().and_then(|s| s.strip_prefix("s=")).unwrap();
        assert_eq!(base64::decode(salt.as_bytes()).unwrap().len(), 16);
        assert_eq!((f.next(), f.next()), (Some("i=4096"), None));
        let fin = c.last(&server_first, None, "");
        let (reply, who) = awaiting.finish(&fin.message).unwrap();
        assert_eq!(
            reply, fin.expected,
            "{kind:?}: the client checks the signature"
        );
        assert_eq!(who.as_str(), "User:alice");
    }
}

/// wrong-password-*, other-users-password-*: the generic text, at client-final.
#[test]
fn a_wrong_password_is_refused_at_the_final_message_with_the_generic_text() {
    for kind in Kind::ALL {
        for password in [
            "nope",
            "bob-secret",
            "alice-secre",
            "alice-secret ",
            "ALICE-SECRET",
        ] {
            let c = Client::new(kind, "alice", password, NC);
            let r = good(kind, &c).unwrap_err();
            assert_eq!(r, rejected(kind, Why::BadProof), "{password}");
            assert!(!r.is_silent());
            assert_eq!(
                r.to_string(),
                format!(
                    "Authentication failed during authentication due to invalid credentials with SASL mechanism {}",
                    kind.mechanism()
                )
            );
        }
    }
}

/// unknown-user-*: refused at client-first, with the same text as a wrong password.
#[test]
fn an_unknown_user_is_refused_at_the_first_message_with_the_same_text() {
    for kind in Kind::ALL {
        for user in ["mallory", "Alice", "alice ", "al=2Cice"] {
            let bare = format!("n,,n={user},r={NC}");
            let r = start(&users(), kind, bare.as_bytes()).unwrap_err();
            assert_eq!(r, rejected(kind, Why::UnknownUser), "{user}");
        }
        // authzid-other-user-unknown-user: the user is looked up first, so the authzid is not mentioned.
        let r = start(&users(), kind, b"n,a=bob,n=mallory,r=NC.").unwrap_err();
        assert_eq!(r, rejected(kind, Why::UnknownUser));
    }
}

/// A user known to the other hash only is unknown to this one.
#[test]
fn the_verifier_of_the_other_hash_is_not_used() {
    let sha256_only = Credentials::new([("alice".to_owned(), "alice-secret".to_owned())])
        .unwrap()
        .with_scram(&[Kind::Sha256])
        .unwrap();
    assert!(start(&sha256_only, Kind::Sha256, &alice(Kind::Sha256).first()).is_ok());
    let r = start(&sha256_only, Kind::Sha512, &alice(Kind::Sha512).first());
    assert_eq!(r.unwrap_err(), rejected(Kind::Sha512, Why::UnknownUser));
}

/// authzid-*: the user's own name is fine; another's is told so, for a user that exists.
#[test]
fn an_authzid_must_be_the_user_and_is_judged_after_the_user_exists() {
    let said = "Authentication failed: Client requested an authorization id that is different from username";
    for kind in Kind::ALL {
        let ok = alice(kind).with_header("n,a=alice,");
        let (_, who) = good(kind, &ok).unwrap(); // authzid-same
        assert_eq!(who.as_str(), "User:alice");
        for other in ["bob", "admin", "Alice", "al=2Cice"] {
            let header = format!("n,a={other},");
            let r = start(
                &users(),
                kind,
                alice(kind).with_header(&header).first().as_slice(),
            );
            assert_eq!(r.unwrap_err(), Refusal::AuthzId, "{other}");
        }
        assert_eq!(Refusal::AuthzId.to_string(), said);
        assert!(!Refusal::AuthzId.is_silent());
    }
}

/// replay-elsewhere: a final message made for another exchange (another server nonce) is refused.
#[test]
fn a_final_message_from_another_exchange_is_refused() {
    let kind = Kind::Sha256;
    let users = users();
    let c = alice(kind);
    let (sf_a, a) = first(kind, &c.first(), "ONE", |u| users.verifier(kind, u)).unwrap();
    let (_, b) = first(kind, &c.first(), "TWO", |u| users.verifier(kind, u)).unwrap();
    let message = c.last(&sf_a, None, "").message;
    assert_eq!(
        b.finish(&message).unwrap_err(),
        rejected(kind, Why::WrongNonce)
    );
    assert!(a.finish(&message).is_ok());
}

#[test]
fn every_user_has_its_own_salt_for_each_hash() {
    let users = users();
    let mut salts = Vec::new();
    for kind in Kind::ALL {
        for user in ["admin", "alice", "bob"] {
            salts.push(users.verifier(kind, user).unwrap().salt().to_vec());
        }
    }
    salts.sort();
    salts.dedup();
    assert_eq!(salts.len(), 6);
    assert!(salts.iter().all(|s| s.len() == 16));
}

#[test]
fn server_nonces_are_fresh_printable_and_without_commas() {
    let mut seen = Vec::new();
    for _ in 0..50 {
        let n = fresh_nonce().unwrap();
        assert_eq!(n.len(), 24);
        assert!(
            n.bytes().all(|b| (0x21..=0x7e).contains(&b) && b != b','),
            "{n}"
        );
        seen.push(n);
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 50);
}

/// Nothing a client sent, no key and no nonce is in what can be printed.
#[test]
fn a_refusal_and_an_exchange_in_flight_print_no_secret() {
    let kind = Kind::Sha256;
    let c = alice(kind);
    let (_, awaiting) = start(&users(), kind, &c.first()).unwrap();
    let shown = format!("{awaiting:?} {awaiting:#?}");
    assert!(shown.contains("Awaiting"), "{shown}");
    for secret in ["alice", "NC.", "SN", "alice-secret"] {
        assert!(!shown.contains(secret), "{secret} in {shown}");
    }
    let user_text = "n,,n=planted-user-3141,r=planted-nonce-2718";
    let r = start(&users(), kind, user_text.as_bytes()).unwrap_err();
    let shown = format!("{r:?} {r}");
    assert!(!shown.contains("planted"), "{shown}");
    let r = awaiting
        .finish(b"c=biws,r=planted-nonce-2718,p=planted")
        .unwrap_err();
    assert!(!format!("{r:?} {r}").contains("planted"));
}

#[test]
fn an_iteration_count_is_what_the_client_is_told() {
    let v = Verifier::derive(Kind::Sha256, b"pw", b"salt", NonZeroU32::new(5000).unwrap());
    let v = Arc::new(v);
    let (sf, _) = first(Kind::Sha256, b"n,,n=u,r=C", "S", |_| Some(v)).unwrap();
    assert_eq!(String::from_utf8(sf).unwrap(), "r=CS,s=c2FsdA==,i=5000");
}

/// A user whose name holds `,` and `=` logs in through the escapes; an authzid is compared as sent (escaped)
/// with the unescaped name, as Kafka does, so even the user's own name in escaped form is refused as an authzid.
#[test]
fn a_name_with_a_comma_and_an_equals_sign_logs_in_by_its_escapes() {
    let pairs = [("a,b=c".to_owned(), "pw".to_owned())];
    let users = Credentials::new(pairs)
        .unwrap()
        .with_scram(&Kind::ALL)
        .unwrap();
    for kind in Kind::ALL {
        let c = Client::new(kind, "a,b=c", "pw", NC);
        assert_eq!(c.first(), b"n,,n=a=2Cb=3Dc,r=NC.");
        let (sf, awaiting) = first(kind, &c.first(), SN, |u| users.verifier(kind, u)).unwrap();
        let (_, who) = awaiting.finish(&c.last(&sf, None, "").message).unwrap();
        assert_eq!(who.as_str(), "User:a,b=c");
        let escaped = c.clone().with_header("n,a=a=2Cb=3Dc,");
        let r = first(kind, &escaped.first(), SN, |u| users.verifier(kind, u));
        assert_eq!(r.unwrap_err(), Refusal::AuthzId);
    }
}

/// Whatever finds the verifier, one of the other hash is never used: a SHA-256 verifier cannot answer a SHA-512 login.
#[test]
fn a_lookup_that_returns_the_other_hashs_verifier_is_not_believed() {
    let users = users();
    let sha256 = users.verifier(Kind::Sha256, "alice").unwrap();
    let r = first(Kind::Sha512, &alice(Kind::Sha512).first(), SN, |_| {
        Some(sha256)
    });
    assert_eq!(r.unwrap_err(), rejected(Kind::Sha512, Why::UnknownUser));
}
