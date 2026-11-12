//! AlterUserScramCredentials, part one: every rule of `ScramControlManager.alterCredentials` (Kafka 4.3.1), each
//! pinned to the line of the source it comes from, and the logins the stored credentials allow.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::sync::Arc;

use support::users::{Line, SHA256, SHA512, bootstrap_users, delete, logs_in, salted, upsert};
use support::{open_on_with, rig_with};

fn ok(user: &str) -> Line {
    (user.to_owned(), 0, None)
}

fn err(user: &str, code: i16, message: Option<&str>) -> Line {
    (user.to_owned(), code, message.map(str::to_owned))
}

const TWICE: &str = "A user credential cannot be altered twice in the same request";

#[tokio::test]
async fn a_user_is_made_for_each_mechanism_and_logs_in_with_the_password_the_client_salted() {
    let users = bootstrap_users(&[]);
    let rig = rig_with(|c| c.users = Arc::clone(&users));
    let r = rig
        .scram_alter(
            vec![],
            vec![
                upsert("bob", SHA256, 4096, "bob-secret"),
                upsert("bob", SHA512, 5000, "bob-secret"),
            ],
        )
        .await;
    // Two upsertions of one user in one request: ScramControlManager.java:211-216.
    assert_eq!(r, vec![err("bob", 92, Some(TWICE))]);
    let r = rig
        .scram_alter(vec![], vec![upsert("bob", SHA256, 4096, "bob-secret")])
        .await;
    assert_eq!(r, vec![ok("bob")]);
    let r = rig
        .scram_alter(vec![], vec![upsert("bob", SHA512, 5000, "bob-secret")])
        .await;
    assert_eq!(r, vec![ok("bob")]);
    assert!(logs_in(&users, SHA256, "bob", "bob-secret"));
    assert!(logs_in(&users, SHA512, "bob", "bob-secret"));
    assert!(!logs_in(&users, SHA512, "bob", "bob-secre"));
    assert_eq!(
        rig.scram_listed().await,
        vec![("bob".to_owned(), vec![(SHA256, 4096), (SHA512, 5000)])]
    );
}

#[tokio::test]
async fn an_upsertion_replaces_the_credential_and_the_old_password_stops_working() {
    let users = bootstrap_users(&[]);
    let rig = rig_with(|c| c.users = Arc::clone(&users));
    rig.scram_alter(vec![], vec![upsert("bob", SHA256, 4096, "one")])
        .await;
    assert!(logs_in(&users, SHA256, "bob", "one"));
    let r = rig
        .scram_alter(vec![], vec![upsert("bob", SHA256, 6000, "two")])
        .await;
    assert_eq!(r, vec![ok("bob")]);
    assert!(!logs_in(&users, SHA256, "bob", "one"));
    assert!(logs_in(&users, SHA256, "bob", "two"));
    assert_eq!(
        rig.scram_listed().await,
        vec![("bob".to_owned(), vec![(SHA256, 6000)])]
    );
}

#[tokio::test]
async fn the_iteration_count_must_be_from_4096_to_16384() {
    // ScramControlManager.java:289-294; MAX_ITERATIONS is 16384 (:58) and the minimum of both mechanisms is 4096.
    let rig = rig_with(|_| {});
    for mechanism in [SHA256, SHA512] {
        for (iterations, want) in [
            (4095, Some("Too few iterations")),
            (4096, None),
            (16384, None),
            (16385, Some("Too many iterations")),
            (0, Some("Too few iterations")),
            (-1, Some("Too few iterations")),
            (i32::MIN, Some("Too few iterations")),
            (i32::MAX, Some("Too many iterations")),
        ] {
            let got = rig
                .scram_alter(vec![], vec![upsert("u", mechanism, iterations, "p")])
                .await;
            let expected = match want {
                Some(m) => err("u", 93, Some(m)),
                None => ok("u"),
            };
            assert_eq!(
                got,
                vec![expected],
                "{iterations} for mechanism {mechanism}"
            );
        }
    }
}

#[tokio::test]
async fn an_empty_name_and_an_unknown_mechanism_are_refused_with_kafkas_words() {
    let rig = rig_with(|_| {});
    // :311-322: the name is judged before the mechanism, for a deletion and for an upsertion.
    let name = Some("Username must not be empty");
    let mech = Some("Unknown SCRAM mechanism");
    assert_eq!(
        rig.scram_alter(vec![delete("", SHA256)], vec![]).await,
        vec![err("", 93, name)]
    );
    assert_eq!(
        rig.scram_alter(vec![delete("", 7)], vec![]).await,
        vec![err("", 93, name)]
    );
    assert_eq!(
        rig.scram_alter(vec![], vec![upsert("", SHA512, 4096, "p")])
            .await,
        vec![err("", 93, name)]
    );
    for bad in [0, 3, -1, 127] {
        assert_eq!(
            rig.scram_alter(vec![delete("x", bad)], vec![]).await,
            vec![err("x", 33, mech)],
            "delete {bad}"
        );
        assert_eq!(
            rig.scram_alter(vec![], vec![upsert("x", bad, 4096, "p")])
                .await,
            vec![err("x", 33, mech)],
            "upsert {bad}"
        );
    }
    // The mechanism is judged before the iteration count (:284-293).
    assert_eq!(
        rig.scram_alter(vec![], vec![upsert("x", 0, 1, "p")]).await,
        vec![err("x", 33, mech)]
    );
    assert!(rig.scram_listed().await.is_empty());
}

#[tokio::test]
async fn deleting_what_is_not_there_is_resource_not_found() {
    let rig = rig_with(|_| {});
    let missing = Some("Attempt to delete a user credential that does not exist");
    assert_eq!(
        rig.scram_alter(vec![delete("ghost", SHA256)], vec![]).await,
        vec![err("ghost", 91, missing)]
    );
    // :298-309: the key is the user and the mechanism, so another mechanism's credential is not this one.
    rig.scram_alter(vec![], vec![upsert("bob", SHA256, 4096, "p")])
        .await;
    assert_eq!(
        rig.scram_alter(vec![delete("bob", SHA512)], vec![]).await,
        vec![err("bob", 91, missing)]
    );
    assert_eq!(
        rig.scram_alter(vec![delete("bob", SHA256)], vec![]).await,
        vec![ok("bob")]
    );
    assert_eq!(
        rig.scram_alter(vec![delete("bob", SHA256)], vec![]).await,
        vec![err("bob", 91, missing)]
    );
}

#[tokio::test]
async fn a_deleted_credential_no_longer_logs_in_and_the_other_mechanism_still_does() {
    let users = bootstrap_users(&[]);
    let rig = rig_with(|c| c.users = Arc::clone(&users));
    rig.scram_alter(vec![], vec![upsert("bob", SHA256, 4096, "p")])
        .await;
    rig.scram_alter(vec![], vec![upsert("bob", SHA512, 4096, "p")])
        .await;
    rig.scram_alter(vec![delete("bob", SHA256)], vec![]).await;
    assert!(!logs_in(&users, SHA256, "bob", "p"));
    assert!(logs_in(&users, SHA512, "bob", "p"));
    assert_eq!(
        rig.scram_listed().await,
        vec![("bob".to_owned(), vec![(SHA512, 4096)])]
    );
    rig.scram_alter(vec![delete("bob", SHA512)], vec![]).await;
    assert!(!logs_in(&users, SHA512, "bob", "p"));
    assert!(rig.scram_listed().await.is_empty());
}

#[tokio::test]
async fn a_user_touched_twice_in_one_request_is_refused_whatever_the_operations() {
    // :188-226: the key is the user name, not the user and mechanism; the earlier operation is cancelled and a third
    // occurrence is ignored.
    let rig = rig_with(|_| {});
    rig.scram_alter(vec![], vec![upsert("bob", SHA256, 4096, "keep")])
        .await;
    let cases: [(Vec<_>, Vec<_>); 5] = [
        (vec![delete("bob", SHA256), delete("bob", SHA256)], vec![]),
        (
            vec![delete("bob", SHA256)],
            vec![upsert("bob", SHA256, 4096, "x")],
        ),
        (
            vec![delete("bob", SHA256)],
            vec![upsert("bob", SHA512, 4096, "x")],
        ),
        (
            vec![],
            vec![
                upsert("bob", SHA256, 4096, "x"),
                upsert("bob", SHA256, 4096, "y"),
            ],
        ),
        (
            vec![delete("bob", SHA256), delete("bob", SHA512)],
            vec![upsert("bob", SHA256, 4096, "z")],
        ),
    ];
    for (i, (deletions, upsertions)) in cases.into_iter().enumerate() {
        let r = rig.scram_alter(deletions, upsertions).await;
        assert_eq!(r, vec![err("bob", 92, Some(TWICE))], "case {i}");
        // The earlier operation was cancelled too: the stored credential is as it was.
        assert_eq!(
            rig.scram_listed().await,
            vec![("bob".to_owned(), vec![(SHA256, 4096)])]
        );
    }
}

#[tokio::test]
async fn a_user_whose_first_operation_failed_keeps_that_error_and_a_repeat_is_ignored() {
    // :189, :209: `userToError.containsKey` skips every later operation on the user.
    let rig = rig_with(|_| {});
    let r = rig
        .scram_alter(
            vec![],
            vec![
                upsert("bob", SHA256, 1, "p"),
                upsert("bob", SHA256, 4096, "p"),
            ],
        )
        .await;
    assert_eq!(r, vec![err("bob", 93, Some("Too few iterations"))]);
    assert!(rig.scram_listed().await.is_empty());
    // A third occurrence after a duplicate is ignored too.
    let r = rig
        .scram_alter(
            vec![],
            vec![
                upsert("u", SHA256, 4096, "p"),
                upsert("u", SHA256, 4096, "p"),
                upsert("u", SHA256, 4096, "p"),
            ],
        )
        .await;
    assert_eq!(r, vec![err("u", 92, Some(TWICE))]);
}

#[tokio::test]
async fn a_mixed_request_answers_deletions_then_upsertions_then_errors() {
    // :229-257.
    let rig = rig_with(|_| {});
    rig.scram_alter(vec![], vec![upsert("old1", SHA256, 4096, "p")])
        .await;
    rig.scram_alter(vec![], vec![upsert("old2", SHA512, 4096, "p")])
        .await;
    let r = rig
        .scram_alter(
            vec![
                delete("ghost", SHA256),
                delete("old2", SHA512),
                delete("old1", SHA256),
            ],
            vec![
                upsert("bad", SHA256, 5, "p"),
                upsert("new1", SHA256, 4096, "p"),
                upsert("new2", SHA512, 4096, "p"),
            ],
        )
        .await;
    assert_eq!(
        r,
        vec![
            ok("old2"),
            ok("old1"),
            ok("new1"),
            ok("new2"),
            err(
                "ghost",
                91,
                Some("Attempt to delete a user credential that does not exist")
            ),
            err("bad", 93, Some("Too few iterations")),
        ]
    );
    assert_eq!(
        rig.scram_listed().await,
        vec![
            ("new1".to_owned(), vec![(SHA256, 4096)]),
            ("new2".to_owned(), vec![(SHA512, 4096)]),
        ]
    );
}

#[tokio::test]
async fn the_salt_and_the_salted_password_are_not_judged_except_that_a_key_cannot_be_empty() {
    let users = bootstrap_users(&[]);
    let rig = rig_with(|c| c.users = Arc::clone(&users));
    // Salt lengths are not validated (kafka-semantics 3.6; no check in :284-296): an empty salt is stored.
    let mut empty_salt = upsert("a", SHA256, 4096, "p");
    empty_salt.salt = bytes::Bytes::new();
    empty_salt.salted_password = salted(SHA256, "p", b"", 4096).into();
    assert_eq!(
        rig.scram_alter(vec![], vec![empty_salt]).await,
        vec![ok("a")]
    );
    assert!(logs_in(&users, SHA256, "a", "p"));
    assert!(
        users
            .verifier(kafka_auth::scram::Kind::Sha256, "a")
            .unwrap()
            .salt()
            .is_empty()
    );
    // A salted password of any length is a key. An empty one is not: Java's SecretKeySpec throws
    // IllegalArgumentException("Empty key") inside `finishUpsertion` (:260-282), which `ApiError.fromThrowable` turns
    // into UNKNOWN_SERVER_ERROR with no message (ApiError.java:35-41), and no record is written.
    let mut short = upsert("b", SHA512, 4096, "p");
    short.salted_password = bytes::Bytes::from_static(b"x");
    let mut none = upsert("c", SHA512, 4096, "p");
    none.salted_password = bytes::Bytes::new();
    let r = rig.scram_alter(vec![], vec![short, none]).await;
    assert_eq!(r, vec![ok("b"), err("c", -1, None)]);
    let names: Vec<_> = rig.scram_listed().await.into_iter().map(|l| l.0).collect();
    assert_eq!(names, ["a", "b"]);
}

#[tokio::test]
async fn an_empty_request_changes_nothing_and_answers_nothing() {
    let rig = rig_with(|_| {});
    assert!(rig.scram_alter(vec![], vec![]).await.is_empty());
}

#[tokio::test]
async fn two_requests_at_once_lose_neither() {
    let users = bootstrap_users(&[]);
    let rig = rig_with(|c| c.users = Arc::clone(&users));
    let (a, b) = tokio::join!(
        rig.scram_alter(vec![], vec![upsert("a", SHA256, 4096, "p")]),
        rig.scram_alter(vec![], vec![upsert("b", SHA256, 4096, "p")]),
    );
    assert_eq!((a, b), (vec![ok("a")], vec![ok("b")]));
    assert!(logs_in(&users, SHA256, "a", "p") && logs_in(&users, SHA256, "b", "p"));
    let again = open_on_with(rig.fs.process_restart(), |_| {});
    let names: Vec<_> = again
        .scram_listed()
        .await
        .into_iter()
        .map(|l| l.0)
        .collect();
    assert_eq!(names, ["a", "b"]);
}
