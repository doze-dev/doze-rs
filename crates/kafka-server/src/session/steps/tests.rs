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
    let (r, fate) = authenticate(&mut s, &authenticator(), &token(b"\0alice\0alice-secret"));
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
        let (r, fate) = authenticate(&mut s, &authenticator(), &token(bytes));
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
        let (r, fate) = authenticate(&mut s, &authenticator(), &token(bytes));
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
    let (r, fate) = authenticate(&mut s, &authenticator(), &token(b"\0alice\0alice-secret"));
    assert_eq!((r.error_code, fate), (34, Fate::Closing));
    assert!(!s.is_authenticated());
}
