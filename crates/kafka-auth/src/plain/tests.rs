//! Every PLAIN step of the recorded `sasl-plain` scenario (docs/port/phase8/go-oracle-rust.md A2.1), with
//! the users the harness configures.

use super::*;

fn users() -> Credentials {
    Credentials::new(
        [
            ("admin", "admin-secret"),
            ("alice", "alice-secret"),
            ("bob", "bob-secret"),
        ]
        .map(|(u, p)| (u.to_owned(), p.to_owned())),
    )
    .unwrap()
}

fn login(token: &[u8]) -> Result<String, String> {
    check(token, &users())
        .map(|p| p.to_string())
        .map_err(|r| r.to_string())
}

const PLAIN_BAD: &str = "Authentication failed: Invalid username or password";
const NO_USER: &str = "Authentication failed: username not specified";
const NO_PASS: &str = "Authentication failed: password not specified";
const AUTHZID: &str =
    "Authentication failed: Client requested an authorization id that is different from username";

fn tokens(n: usize) -> String {
    format!("Invalid SASL/PLAIN response: expected 3 tokens, got {n}")
}

#[test]
fn the_right_password_logs_in_as_the_user() {
    assert_eq!(login(b"\0alice\0alice-secret"), Ok("User:alice".into())); // good
    assert_eq!(login(b"\0bob\0bob-secret"), Ok("User:bob".into())); // good-bob
    assert_eq!(login(b"\0admin\0admin-secret"), Ok("User:admin".into()));
}

#[test]
fn an_authzid_that_is_the_user_is_fine_and_any_other_is_refused_after_the_password() {
    assert_eq!(
        login(b"alice\0alice\0alice-secret"),
        Ok("User:alice".into())
    ); // authzid-same
    for other in ["bob", "admin", "mallory"] {
        let token = format!("{other}\0alice\0alice-secret");
        assert_eq!(login(token.as_bytes()), Err(AUTHZID.into()), "{other}");
    }
    // The password is judged first: a wrong or empty password hides the authzid.
    assert_eq!(login(b"bob\0alice\0nope"), Err(PLAIN_BAD.into()));
    assert_eq!(login(b"bob\0alice\0"), Err(NO_PASS.into()));
}

#[test]
fn a_wrong_password_in_any_way_is_one_refusal() {
    let wrong: [&[u8]; 8] = [
        b"\0alice\0nope",
        b"\0alice\0bob-secret",
        b"\0alice\0alice-secre",
        b"\0alice\0alice-secret1",
        b"\0mallory\0alice-secret",
        b"\0ALICE\0alice-secret",
        b"\0alice\0ALICE-SECRET",
        b"\0 alice\0alice-secret",
    ];
    for token in wrong {
        assert_eq!(login(token), Err(PLAIN_BAD.into()), "{token:?}");
    }
}

#[test]
fn empty_parts_are_named_user_first() {
    assert_eq!(login(b"\0alice\0"), Err(NO_PASS.into())); // empty-password
    assert_eq!(login(b"\0\0alice-secret"), Err(NO_USER.into())); // empty-user
    assert_eq!(login(b"\0\0"), Err(NO_USER.into())); // empty-user-and-password
}

#[test]
fn the_token_must_have_exactly_three_parts() {
    assert_eq!(login(b"alice alice-secret"), Err(tokens(1))); // no-nuls
    assert_eq!(login(b"alice\0alice-secret"), Err(tokens(2))); // two-parts
    assert_eq!(login(b"\0alice\0alice-secret\0extra"), Err(tokens(4))); // four-parts
    assert_eq!(login(b"\0alice\0alice-secret\0"), Err(tokens(4))); // trailing-nul
    assert_eq!(login(b""), Err(tokens(1))); // empty-token
    // Kafka stops at four parts, however many more NULs there are.
    assert_eq!(login(b"\0\0\0\0\0\0\0"), Err(tokens(4)));
}

#[test]
fn bytes_that_are_not_text_are_a_login_that_matches_nobody() {
    assert_eq!(login(b"\0alice\0\xff\xfe\xfd"), Err(PLAIN_BAD.into())); // not-utf8
    assert_eq!(login(b"\0\xff\xfe\0alice-secret"), Err(PLAIN_BAD.into()));
}

#[test]
fn a_token_of_64_kib_is_judged_like_any_other() {
    let mut token = b"\0alice\0".to_vec();
    token.extend(std::iter::repeat_n(b'x', 65_536));
    assert_eq!(login(&token), Err(PLAIN_BAD.into())); // token-of-64k
}

#[test]
fn no_refusal_repeats_what_the_client_sent() {
    let sent = [
        &b"\0mallory-planted\0pw-planted"[..],
        b"zz-planted\0alice\0pw-planted",
        b"\0alice-planted\0",
        b"a-planted b-planted",
    ];
    for token in sent {
        let said = login(token).unwrap_err();
        assert!(!said.contains("planted"), "{said}");
    }
}

#[test]
fn the_texts_are_kafkas_own() {
    assert_eq!(Refusal::BadCredentials.to_string(), PLAIN_BAD);
    assert_eq!(Refusal::NoUser.to_string(), NO_USER);
    assert_eq!(Refusal::NoPassword.to_string(), NO_PASS);
    assert_eq!(Refusal::AuthzId.to_string(), AUTHZID);
    assert_eq!(Refusal::Tokens { got: 3 }.to_string(), tokens(3));
}
