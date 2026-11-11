//! The answers of `sasl-plain`, step by step (docs/port/phase8/go-oracle-rust.md A2.1).

use kafka_auth::{Credentials, Principal};

use super::*;

fn authenticator() -> Authenticator {
    let users = [("alice", "alice-secret"), ("bob", "bob-secret")]
        .map(|(u, p)| (u.to_owned(), p.to_owned()));
    Authenticator::new(&[Mechanism::Plain], Credentials::new(users).unwrap()).unwrap()
}

fn hs(mechanism: &'static str) -> SaslHandshakeRequest {
    let mut r = SaslHandshakeRequest::default();
    r.mechanism = StrBytes::from_static_str(mechanism);
    r
}

fn token(bytes: &[u8]) -> SaslAuthenticateRequest {
    let mut r = SaslAuthenticateRequest::default();
    r.auth_bytes = Bytes::copy_from_slice(bytes);
    r
}

fn handshaken() -> Session {
    Session::Handshaken(Mechanism::Plain)
}

fn names(r: &SaslHandshakeResponse) -> Vec<&str> {
    r.mechanisms.iter().map(|m| &**m).collect()
}

fn message(r: &SaslAuthenticateResponse) -> Option<&str> {
    r.error_message.as_deref()
}

#[test]
fn a_handshake_for_an_offered_mechanism_is_answered_with_the_list_and_moves_on() {
    let mut s = Session::new();
    let (r, fate) = handshake(&mut s, &authenticator(), &hs("PLAIN"));
    assert_eq!(
        (r.error_code, names(&r), fate),
        (0, vec!["PLAIN"], Fate::Open)
    );
    assert!(matches!(s, Session::Handshaken(Mechanism::Plain)));
}

#[test]
fn a_mechanism_that_is_not_offered_is_33_with_the_list_and_then_closed() {
    // unknown-mechanism, lowercase-mechanism, empty-mechanism, scram-not-enabled
    for wrong in [
        "BOGUS",
        "plain",
        "",
        "SCRAM-SHA-256",
        "SCRAM-SHA-512",
        "PLAIN ",
    ] {
        let mut s = Session::new();
        let (r, fate) = handshake(&mut s, &authenticator(), &hs(wrong));
        assert_eq!(
            (r.error_code, names(&r), fate),
            (33, vec!["PLAIN"], Fate::Closing),
            "{wrong:?}"
        );
        assert!(matches!(s, Session::Unauthenticated { .. }), "{wrong:?}");
    }
}

#[test]
fn a_handshake_in_any_other_state_is_34_with_no_mechanisms_and_changes_nothing() {
    // handshake-twice, and `twice` after a login.
    let mut s = handshaken();
    let (r, fate) = handshake(&mut s, &authenticator(), &hs("PLAIN"));
    assert_eq!(
        (r.error_code, r.mechanisms.len(), fate),
        (34, 0, Fate::Closing)
    );
    assert!(matches!(s, Session::Handshaken(_)));
    assert_eq!(illegal_handshake().error_code, 34);
    assert!(illegal_handshake().mechanisms.is_empty());
}

#[test]
fn the_right_token_logs_in_with_an_empty_message_not_an_absent_one() {
    let mut s = handshaken();
    let (r, fate) =
        authenticate(&mut s, &authenticator(), &token(b"\0alice\0alice-secret")).unwrap();
    assert_eq!((r.error_code, message(&r), fate), (0, Some(""), Fate::Open));
    assert!(r.auth_bytes.is_empty() && r.session_lifetime_ms == 0);
    assert_eq!(s.principal().map(Principal::as_str), Some("User:alice"));
}

#[test]
fn every_recorded_refusal_is_58_with_kafkas_text_and_closes() {
    let cases: [(&[u8], &str); 5] = [
        (
            b"\0alice\0nope",
            "Authentication failed: Invalid username or password",
        ),
        (
            b"\0alice\0",
            "Authentication failed: password not specified",
        ),
        (
            b"\0\0alice-secret",
            "Authentication failed: username not specified",
        ),
        (
            b"alice alice-secret",
            "Invalid SASL/PLAIN response: expected 3 tokens, got 1",
        ),
        (
            b"bob\0alice\0alice-secret",
            "Authentication failed: Client requested an authorization id that is different from username",
        ),
    ];
    for (bytes, text) in cases {
        let mut s = handshaken();
        let (r, fate) = authenticate(&mut s, &authenticator(), &token(bytes)).unwrap();
        assert_eq!(
            (r.error_code, message(&r), fate),
            (58, Some(text), Fate::Closing)
        );
        assert!(r.auth_bytes.is_empty());
        assert!(!s.is_authenticated(), "{bytes:?} must not log in");
    }
}

#[test]
fn a_second_token_after_a_login_is_34_whoever_it_names_and_changes_nobody() {
    // twice: the same token, another user's valid token, a wrong password, an empty token.
    let after = "SaslAuthenticate request received after successful authentication";
    for bytes in [
        &b"\0alice\0alice-secret"[..],
        b"\0bob\0bob-secret",
        b"\0alice\0nope",
        b"",
    ] {
        let mut s = Session::Authenticated(Principal::user("alice"));
        let (r, fate) = authenticate(&mut s, &authenticator(), &token(bytes)).unwrap();
        assert_eq!(
            (r.error_code, message(&r), fate),
            (34, Some(after), Fate::Open)
        );
        assert_eq!(s.principal().map(Principal::as_str), Some("User:alice"));
    }
    assert_eq!(after_login().error_code, 34);
}

#[test]
fn a_token_before_the_handshake_logs_nobody_in() {
    let mut s = Session::new();
    let (r, fate) =
        authenticate(&mut s, &authenticator(), &token(b"\0alice\0alice-secret")).unwrap();
    assert_eq!((r.error_code, fate), (34, Fate::Closing));
    assert!(!s.is_authenticated());
}

// SCRAM: the steps of `sasl-scram` (docs/port/phase8/go-oracle-rust.md A2.2) at the level of a session.

mod scram {
    use kafka_auth::scram::Kind;
    use kafka_auth::scram::client::Client;

    use super::*;

    const BAD: &str = "Authentication failed during authentication due to invalid credentials with SASL mechanism ";

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
}
