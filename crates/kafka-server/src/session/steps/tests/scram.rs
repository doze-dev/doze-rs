//! The steps of `sasl-scram` (docs/port/phase8/go-oracle-rust.md A2.2) at the level of a session.

use kafka_auth::scram::Kind;
use kafka_auth::scram::client::Client;

use super::*;

const BAD: &str =
    "Authentication failed during authentication due to invalid credentials with SASL mechanism ";

fn all() -> Authenticator {
    let users = [("alice", "alice-secret"), ("bob", "bob-secret")]
        .map(|(u, p)| (u.to_owned(), p.to_owned()));
    let all = [
        Mechanism::Plain,
        Mechanism::ScramSha512,
        Mechanism::ScramSha256,
    ];
    Authenticator::new(&all, Credentials::new(users).unwrap()).unwrap()
}

const BOTH: [(Mechanism, Kind); 2] = [
    (Mechanism::ScramSha256, Kind::Sha256),
    (Mechanism::ScramSha512, Kind::Sha512),
];

/// The handshake, then the first token: the session and the answer to the challenge.
fn challenged(m: Mechanism, first: &[u8]) -> (Session, SaslAuthenticateResponse) {
    let mut s = Session::new();
    let name = m.name();
    let (r, fate) = handshake(&mut s, &all(), &hs(name));
    assert_eq!((r.error_code, fate), (0, Fate::Open));
    let (r, fate) = authenticate(&mut s, &all(), &token(first)).unwrap();
    assert_eq!(
        fate,
        if r.error_code == 0 {
            Fate::Open
        } else {
            Fate::Closing
        }
    );
    (s, r)
}

#[test]
fn the_handshake_lists_the_mechanisms_in_javas_order_and_accepts_each_scram_name() {
    for (m, _) in BOTH {
        let mut s = Session::new();
        let (r, fate) = handshake(&mut s, &all(), &hs(m.name()));
        assert_eq!(
            (r.error_code, names(&r), fate),
            (
                0,
                vec!["PLAIN", "SCRAM-SHA-512", "SCRAM-SHA-256"],
                Fate::Open
            )
        );
        assert!(matches!(s, Session::Handshaken(got) if got == m));
    }
}

/// good-256, good-512, and the v0/v1/v2 variants are the same bytes.
#[test]
fn two_tokens_log_a_scram_user_in_with_the_challenge_and_the_signature_as_auth_bytes() {
    for (m, kind) in BOTH {
        let c = Client::new(kind, "alice", "alice-secret", "NC.");
        let (mut s, r) = challenged(m, &c.first());
        assert_eq!(
            (r.error_code, message(&r), r.session_lifetime_ms),
            (0, Some(""), 0)
        );
        let challenge = r.auth_bytes.clone();
        assert!(challenge.starts_with(b"r=NC."), "{challenge:?}");
        assert!(matches!(s, Session::Challenged(_)) && !s.is_authenticated());
        let fin = c.last(&challenge, None, "");
        let (r, fate) = authenticate(&mut s, &all(), &token(&fin.message)).unwrap();
        assert_eq!((r.error_code, message(&r), fate), (0, Some(""), Fate::Open));
        assert_eq!(r.auth_bytes.as_ref(), fin.expected.as_slice());
        assert_eq!(r.session_lifetime_ms, 0);
        assert_eq!(s.principal().map(Principal::as_str), Some("User:alice"));
    }
}

/// wrong-password-*: 58 with the mechanism named, then closed.
#[test]
fn a_wrong_password_is_58_naming_the_mechanism_and_closes() {
    for (m, kind) in BOTH {
        let c = Client::new(kind, "alice", "nope", "NC.");
        let (mut s, r) = challenged(m, &c.first());
        let fin = c.last(&r.auth_bytes, None, "");
        let (r, fate) = authenticate(&mut s, &all(), &token(&fin.message)).unwrap();
        let text = format!("{BAD}{m}");
        assert_eq!(
            (r.error_code, message(&r), fate),
            (58, Some(text.as_str()), Fate::Closing)
        );
        assert!(r.auth_bytes.is_empty() && !s.is_authenticated());
    }
}

/// unknown-user-*, authzid-*, first-*: refused at the first message.
#[test]
fn a_refused_first_message_is_58_and_closes_before_a_challenge() {
    let authzid = "Authentication failed: Client requested an authorization id that is different from username";
    for (m, _) in BOTH {
        let cases: [(&[u8], String); 5] = [
            (b"n,,n=mallory,r=NC.", format!("{BAD}{m}")),
            (b"", format!("{BAD}{m}")),
            (b"y,,n=alice,r=NC.", format!("{BAD}{m}")),
            (b"n,a=bob,n=mallory,r=NC.", format!("{BAD}{m}")),
            (b"n,a=bob,n=alice,r=NC.", authzid.to_owned()),
        ];
        for (first, text) in cases {
            let (s, r) = challenged(m, first);
            assert_eq!((r.error_code, message(&r)), (58, Some(text.as_str())));
            assert!(r.auth_bytes.is_empty() && matches!(s, Session::Handshaken(_)));
        }
    }
}

/// final-proof-too-short, final-proof-empty, proof-of-512-under-256: nothing is written.
#[test]
fn a_proof_the_wrong_length_hangs_up_without_an_answer() {
    for proof in ["AAAA", ""] {
        let (mut s, r) = challenged(Mechanism::ScramSha256, b"n,,n=alice,r=NC.");
        let challenge = String::from_utf8(r.auth_bytes.to_vec()).unwrap();
        let nonce = challenge.split(',').next().unwrap();
        let last = format!("c=biws,{nonce},p={proof}");
        assert_eq!(
            authenticate(&mut s, &all(), &token(last.as_bytes())),
            Err(Hangup)
        );
        assert!(!s.is_authenticated());
    }
}

/// final-without-first, plain-token-as-final, first-twice: a token in the wrong place is the generic 58.
#[test]
fn a_token_of_the_wrong_kind_for_the_step_is_58() {
    let text = format!("{BAD}SCRAM-SHA-256");
    let mut s = Session::Handshaken(Mechanism::ScramSha256);
    let (r, fate) = authenticate(&mut s, &all(), &token(b"c=biws,r=NC.,p=AAAA")).unwrap();
    assert_eq!(
        (r.error_code, message(&r), fate),
        (58, Some(text.as_str()), Fate::Closing)
    );
    for second in [&b"n,,n=alice,r=NC."[..], b"\0alice\0alice-secret", b""] {
        let (mut s, _) = challenged(Mechanism::ScramSha256, b"n,,n=alice,r=NC.");
        let (r, fate) = authenticate(&mut s, &all(), &token(second)).unwrap();
        assert_eq!(
            (r.error_code, message(&r), fate),
            (58, Some(text.as_str()), Fate::Closing)
        );
    }
}

/// switch-to-plain, switch-to-512: a handshake in the middle is 34 with no list, and closes.
#[test]
fn a_handshake_in_the_middle_of_a_scram_login_is_34_and_closes() {
    for name in ["PLAIN", "SCRAM-SHA-512", "SCRAM-SHA-256", "BOGUS"] {
        let (mut s, _) = challenged(Mechanism::ScramSha256, b"n,,n=alice,r=NC.");
        let (r, fate) = handshake(&mut s, &all(), &hs(name));
        assert_eq!(
            (r.error_code, r.mechanisms.len(), fate),
            (34, 0, Fate::Closing)
        );
    }
}

/// replay, third-message: after a SCRAM login a token is 34 and the user stays.
#[test]
fn a_token_after_a_scram_login_is_34() {
    let c = Client::new(Kind::Sha512, "alice", "alice-secret", "NC.");
    let (mut s, r) = challenged(Mechanism::ScramSha512, &c.first());
    let fin = c.last(&r.auth_bytes, None, "");
    authenticate(&mut s, &all(), &token(&fin.message)).unwrap();
    for again in [&fin.message[..], b"n,,n=bob,r=NC.", b""] {
        let (r, fate) = authenticate(&mut s, &all(), &token(again)).unwrap();
        assert_eq!((r.error_code, fate), (34, Fate::Open));
        assert_eq!(
            message(&r),
            Some("SaslAuthenticate request received after successful authentication")
        );
        assert_eq!(s.principal().map(Principal::as_str), Some("User:alice"));
    }
}

#[test]
fn a_session_in_flight_prints_no_nonce() {
    let (s, _) = challenged(Mechanism::ScramSha256, b"n,,n=alice,r=planted-nonce-77.");
    let shown = format!("{s:?}");
    assert!(
        shown.contains("Challenged") && !shown.contains("planted") && !shown.contains("alice"),
        "{shown}"
    );
}
