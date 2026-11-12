//! `Session::admit` over every state and every API the server knows: the recorded order of a login.

use kafka_wire::api::API_TABLE;

use super::*;

const HANDSHAKEN: fn() -> Session = || Session::Handshaken(Mechanism::Plain);
fn challenged() -> Session {
    let users = [("alice".to_owned(), "pw".to_owned())];
    let auth = kafka_auth::Authenticator::new(
        &[Mechanism::ScramSha256],
        kafka_auth::Credentials::new(users).unwrap(),
    )
    .unwrap();
    let (_, awaiting) = auth
        .scram_first(Mechanism::ScramSha256, b"n,,n=alice,r=NC.")
        .unwrap();
    Session::Challenged(awaiting)
}

const LOGGED_IN: fn() -> Session = || Session::Authenticated(Principal::user("alice"));

fn others() -> impl Iterator<Item = ApiKey> {
    API_TABLE.iter().map(|r| r.key).filter(|k| {
        !matches!(
            k,
            ApiKey::ApiVersions | ApiKey::SaslHandshake | ApiKey::SaslAuthenticate
        )
    })
}

#[test]
fn a_stranger_may_ask_apiversions_once_and_then_handshake_and_nothing_else() {
    let fresh = Session::new();
    assert_eq!(fresh.admit(ApiKey::ApiVersions), Verdict::Serve);
    assert_eq!(fresh.admit(ApiKey::SaslHandshake), Verdict::Serve);
    // versions-twice: the second ApiVersions is closed unanswered.
    let mut seen = Session::new();
    seen.versions_answered();
    assert_eq!(seen.admit(ApiKey::ApiVersions), Verdict::Close);
    assert_eq!(seen.admit(ApiKey::SaslHandshake), Verdict::Serve);
    // no-handshake: a token before any handshake is closed; so is every other API, in either state.
    for state in [&fresh, &seen] {
        assert_eq!(state.admit(ApiKey::SaslAuthenticate), Verdict::Close);
        for key in others() {
            assert_eq!(state.admit(key), Verdict::Close, "{key:?}");
        }
    }
}

#[test]
fn after_the_handshake_only_a_token_is_welcome_and_two_apis_are_answered_then_closed() {
    // Both rounds of a SCRAM login are in this state of the order: handshaken, then challenged.
    for s in [HANDSHAKEN(), challenged()] {
        assert_after_the_handshake(&s);
        assert!(!s.is_authenticated() && s.principal().is_none());
    }
}

fn assert_after_the_handshake(s: &Session) {
    assert_eq!(s.admit(ApiKey::SaslAuthenticate), Verdict::Serve);
    // handshake-twice, versions-before-token
    for key in [ApiKey::SaslHandshake, ApiKey::ApiVersions] {
        assert_eq!(s.admit(key), Verdict::IllegalState { stays_open: false });
    }
    // Every other request closes the connection (Kafka answers Metadata in its own shape; see SEC-5).
    for key in others() {
        assert_eq!(s.admit(key), Verdict::Close, "{key:?}");
    }
}

#[test]
fn a_logged_in_connection_is_served_everything_but_the_two_sasl_apis() {
    let s = LOGGED_IN();
    for key in others().chain([ApiKey::ApiVersions]) {
        assert_eq!(s.admit(key), Verdict::Serve, "{key:?}");
    }
    // twice: answered 34 and the connection stays.
    for key in [ApiKey::SaslHandshake, ApiKey::SaslAuthenticate] {
        assert_eq!(s.admit(key), Verdict::IllegalState { stays_open: true });
    }
}

#[test]
fn only_a_login_makes_a_connection_authenticated_and_gives_it_a_principal() {
    assert!(!Session::new().is_authenticated() && Session::new().principal().is_none());
    assert!(!HANDSHAKEN().is_authenticated() && HANDSHAKEN().principal().is_none());
    let s = LOGGED_IN();
    assert!(s.is_authenticated());
    assert_eq!(s.principal().map(Principal::as_str), Some("User:alice"));
}

#[test]
fn noting_an_apiversions_changes_only_the_state_before_the_handshake() {
    let mut s = HANDSHAKEN();
    s.versions_answered();
    assert!(matches!(s, Session::Handshaken(Mechanism::Plain)));
    let mut s = LOGGED_IN();
    s.versions_answered();
    assert!(s.is_authenticated());
}
