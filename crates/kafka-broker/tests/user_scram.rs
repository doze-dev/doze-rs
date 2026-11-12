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
