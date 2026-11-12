use std::num::NonZeroU32;

use super::*;
use crate::scram;
use crate::scram::client::Client;

fn boot() -> Users {
    let c = Credentials::new([("alice".to_owned(), "alice-secret".to_owned())]).unwrap();
    Users::derive(c, &[Mechanism::Plain, Mechanism::ScramSha256]).unwrap()
}

fn verifier(kind: Kind, password: &str, salt: &[u8], iterations: u32) -> Verifier {
    let salted = kind.salted_password(
        password.as_bytes(),
        salt,
        NonZeroU32::new(iterations).unwrap(),
    );
    Verifier::from_salted_password(kind, salt, NonZeroU32::new(iterations).unwrap(), &salted)
}

fn logs_in(users: &Users, kind: Kind, user: &str, password: &str) -> bool {
    let client = Client::new(kind, user, password, "nonce.");
    let Some(v) = users.verifier(kind, user) else {
        return false;
    };
    let Ok((server_first, awaiting)) = scram::first(kind, &client.first(), "SN", |_| Some(v))
    else {
        return false;
    };
    let fin = client.last(&server_first, None, "");
    awaiting.finish(&fin.message).is_ok()
}

#[test]
fn a_bootstrap_user_logs_in_only_for_the_mechanisms_that_are_on() {
    let users = boot();
    assert!(logs_in(&users, Kind::Sha256, "alice", "alice-secret"));
    assert!(!logs_in(&users, Kind::Sha256, "alice", "nope"));
    assert!(users.verifier(Kind::Sha512, "alice").is_none());
    assert!(users.is_bootstrap("alice") && !users.is_bootstrap("bob"));
    assert!(users.verifier(Kind::Sha256, "bob").is_none());
}

#[test]
fn a_stored_user_logs_in_with_the_verifier_that_was_swapped_in() {
    let users = boot();
    assert!(users.stored().is_empty());
    let stored = users.stored().with(
        "bob",
        verifier(Kind::Sha512, "bob-secret", b"salt-of-bob", 5000),
    );
    users.replace_stored(stored);
    assert!(logs_in(&users, Kind::Sha512, "bob", "bob-secret"));
    assert!(!logs_in(&users, Kind::Sha512, "bob", "alice-secret"));
    // Another mechanism is another credential.
    assert!(!logs_in(&users, Kind::Sha256, "bob", "bob-secret"));
    let v = users.verifier(Kind::Sha512, "bob").unwrap();
    assert_eq!(
        (v.salt(), v.iterations().get()),
        (&b"salt-of-bob"[..], 5000)
    );
}

#[test]
fn a_user_in_both_is_the_bootstrap_one_whatever_is_stored() {
    let users = boot();
    users.replace_stored(users.stored().with(
        "alice",
        verifier(Kind::Sha256, "stored-secret", b"salt", 4096),
    ));
    assert!(logs_in(&users, Kind::Sha256, "alice", "alice-secret"));
    assert!(!logs_in(&users, Kind::Sha256, "alice", "stored-secret"));
    // The stored entry for a mechanism the bootstrap user has none of is shadowed too: one name, one source.
    users.replace_stored(users.stored().with(
        "alice",
        verifier(Kind::Sha512, "stored-secret", b"salt", 4096),
    ));
    assert!(users.verifier(Kind::Sha512, "alice").is_none());
}

#[test]
fn a_removal_applies_to_the_next_lookup_and_an_old_snapshot_is_unchanged() {
    let users = boot();
    users.replace_stored(
        users
            .stored()
            .with("bob", verifier(Kind::Sha256, "bob-secret", b"salt", 4096)),
    );
    let before = users.stored();
    users.replace_stored(users.stored().without("bob", Kind::Sha256));
    assert!(users.verifier(Kind::Sha256, "bob").is_none());
    assert!(users.stored().is_empty());
    assert!(before.verifier(Kind::Sha256, "bob").is_some());
    // Removing what is not there changes nothing.
    users.replace_stored(users.stored().without("nobody", Kind::Sha256));
}

#[test]
fn the_credentials_listed_are_both_sources_sorted_with_the_bootstrap_one_winning() {
    let users = boot();
    let s = users
        .stored()
        .with("zed", verifier(Kind::Sha512, "p", b"s", 6000))
        .with("zed", verifier(Kind::Sha256, "p", b"s", 7000))
        .with("alice", verifier(Kind::Sha512, "p", b"s", 9000))
        .with("bob", verifier(Kind::Sha256, "p", b"s", 4096));
    users.replace_stored(s);
    assert_eq!(
        users.scram_credentials(),
        vec![
            ("alice".to_owned(), vec![(Kind::Sha256, 4096)]),
            ("bob".to_owned(), vec![(Kind::Sha256, 4096)]),
            (
                "zed".to_owned(),
                vec![(Kind::Sha256, 7000), (Kind::Sha512, 6000)]
            ),
        ]
    );
    assert_eq!(users.scram_of("zed").len(), 2);
    assert!(users.scram_of("nobody").is_empty());
}

#[test]
fn emptiness_counts_both_sources() {
    let none = Users::none();
    assert!(none.is_empty());
    none.replace_stored(
        none.stored()
            .with("bob", verifier(Kind::Sha256, "p", b"s", 4096)),
    );
    assert!(!none.is_empty());
    assert!(!boot().is_empty());
}

#[test]
fn a_verifier_made_from_parts_must_have_keys_the_size_of_the_hash() {
    let v = verifier(Kind::Sha256, "p", b"s", 4096);
    let (stored, server) = v.keys();
    let back = |k: &[u8], s: &[u8]| {
        Verifier::from_parts(Kind::Sha256, b"s", NonZeroU32::new(4096).unwrap(), k, s)
    };
    assert_eq!(back(stored, server).unwrap(), v);
    assert!(back(&stored[1..], server).is_none());
    assert!(back(stored, &[]).is_none());
    assert!(Verifier::from_parts(Kind::Sha512, b"s", NonZeroU32::MIN, stored, server).is_none());
}

#[test]
fn debug_output_names_users_and_shows_no_key() {
    let users = boot();
    users.replace_stored(users.stored().with(
        "bob",
        verifier(Kind::Sha256, "planted-secret", b"planted-salt", 4096),
    ));
    let shown = format!("{users:?} {:#?} {:?}", users.stored(), users.stored());
    assert!(shown.contains("alice") && shown.contains("bob"));
    for planted in ["planted", "alice-secret"] {
        assert!(!shown.contains(planted), "{shown}");
    }
}

#[test]
fn plaintext_is_kept_only_when_plain_is_offered() {
    // SEC-D59: with SCRAM alone the passwords are forgotten once the verifiers are made.
    let pairs = || [("alice".to_owned(), "alice-secret".to_owned())];
    let scram_only = Users::derive(
        Credentials::new(pairs()).unwrap(),
        &[Mechanism::ScramSha256, Mechanism::ScramSha512],
    )
    .unwrap();
    assert!(scram_only.bootstrap().password(b"alice").is_none());
    assert!(scram_only.is_bootstrap("alice") && scram_only.bootstrap().len() == 1);
    assert!(scram_only.bootstrap().has_user("alice") && !scram_only.bootstrap().is_empty());
    assert!(logs_in(&scram_only, Kind::Sha256, "alice", "alice-secret"));
    assert!(logs_in(&scram_only, Kind::Sha512, "alice", "alice-secret"));
    assert!(!logs_in(&scram_only, Kind::Sha256, "alice", "wrong"));
    assert_eq!(
        scram_only.bootstrap().names().collect::<Vec<_>>(),
        ["alice"]
    );
    let with_plain = Users::derive(
        Credentials::new(pairs()).unwrap(),
        &[Mechanism::Plain, Mechanism::ScramSha256],
    )
    .unwrap();
    assert!(with_plain.bootstrap().password(b"alice").is_some());
}
