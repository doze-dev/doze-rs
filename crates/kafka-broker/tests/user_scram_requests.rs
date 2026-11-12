//! AlterUserScramCredentials, part two: a request that touches a user twice, mixes operations, or carries odd salts, and
//! the order of the answer (`ScramControlManager.alterCredentials`, Kafka 4.3.1).
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
async fn a_deletion_that_failed_ends_the_users_part_of_the_request() {
    // :189, :209: the first error stands and every later operation on the name, of either kind, is ignored.
    let rig = rig_with(|_| {});
    let missing = "Attempt to delete a user credential that does not exist";
    let r = rig
        .scram_alter(
            vec![delete("ghost", SHA256), delete("ghost", SHA512)],
            vec![upsert("ghost", SHA256, 4096, "p")],
        )
        .await;
    assert_eq!(r, vec![err("ghost", 91, Some(missing))]);
    assert!(rig.scram_listed().await.is_empty());
}
