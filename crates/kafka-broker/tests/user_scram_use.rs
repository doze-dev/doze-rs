//! The two SCRAM credential APIs, part two: DescribeUserScramCredentials, who may call either, the users configured at
//! start, and what a restart or a failed write does.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::sync::Arc;

use kafka_storage::sim::Failure;
use support::acls::{
    ALLOW, ALTER, CLUSTER, DESCRIBE, LITERAL, admin, closed_world, creation, user,
};
use support::users::{Line, SHA256, SHA512, bootstrap_users, delete, logs_in, upsert};
use support::{open_on_with, rig_with};

fn ok(user: &str) -> Line {
    (user.to_owned(), 0, None)
}

fn err(user: &str, code: i16, message: &str) -> Line {
    (user.to_owned(), code, Some(message.to_owned()))
}

#[tokio::test]
async fn describe_lists_every_user_with_its_mechanisms_and_iterations_and_nothing_secret() {
    let rig = rig_with(|_| {});
    rig.scram_alter(vec![], vec![upsert("zed", SHA512, 7000, "p")])
        .await;
    rig.scram_alter(vec![], vec![upsert("amy", SHA256, 4096, "p")])
        .await;
    rig.scram_alter(vec![], vec![upsert("amy", SHA512, 9000, "p")])
        .await;
    // No list and an empty list both mean everybody (ScramImage.java:84-89).
    let expected = vec![
        ("amy".to_owned(), vec![(SHA256, 4096), (SHA512, 9000)]),
        ("zed".to_owned(), vec![(SHA512, 7000)]),
    ];
    assert_eq!(rig.scram_listed().await, expected);
    let all = rig.scram_describe_as(&admin(), Some(&[])).await;
    assert_eq!(all.results.len(), 2);
    assert!(
        all.results
            .iter()
            .all(|r| r.error_code == 0 && r.error_message.as_deref() == Some(""))
    );
}

#[tokio::test]
async fn describe_by_name_answers_each_name_once_with_kafkas_words_for_the_failures() {
    let rig = rig_with(|_| {});
    rig.scram_alter(vec![], vec![upsert("amy", SHA256, 4096, "p")])
        .await;
    rig.scram_alter(vec![], vec![upsert("bob", SHA512, 5000, "p")])
        .await;
    let r = rig
        .scram_describe_as(&admin(), Some(&["bob", "ghost", "amy", "bob", "bob", ""]))
        .await;
    assert_eq!((r.error_code, r.error_message.as_deref()), (0, Some("")));
    let lines: Vec<_> = r
        .results
        .iter()
        .map(|x| {
            (
                x.user.to_string(),
                x.error_code,
                x.error_message.as_ref().map(ToString::to_string),
                x.credential_infos
                    .iter()
                    .map(|i| (i.mechanism, i.iterations))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    // ScramImage.java:96-127: a name repeated is one entry, DUPLICATE_RESOURCE, whatever the other occurrences
    // are; a name with no credential is RESOURCE_NOT_FOUND naming it. The order is a hash map's there; here the
    // order of first appearance.
    let dup = "Cannot describe SCRAM credentials for the same user twice in a single request: bob";
    let missing =
        |u: &str| format!("Attempt to describe a user credential that does not exist: {u}");
    assert_eq!(
        lines,
        vec![
            ("bob".to_owned(), 92, Some(dup.to_owned()), vec![]),
            ("ghost".to_owned(), 91, Some(missing("ghost")), vec![]),
            (
                "amy".to_owned(),
                0,
                Some(String::new()),
                vec![(SHA256, 4096)]
            ),
            (String::new(), 91, Some(missing("")), vec![]),
        ]
    );
}

#[tokio::test]
async fn without_an_authorizer_anybody_may_manage_users_the_open_broker_posture() {
    // Decision D4: no SASL, ANONYMOUS, `--acl-default allow` and no ACLs: nothing refuses. Documented as SEC-D51.
    let rig = rig_with(|_| {});
    let anonymous = support::caller();
    let r = rig
        .scram_alter_as(&anonymous, vec![], vec![upsert("x", SHA256, 4096, "p")])
        .await;
    assert_eq!(r, vec![ok("x")]);
    assert_eq!(
        rig.scram_describe_as(&anonymous, None).await.results.len(),
        1
    );
}

#[tokio::test]
async fn a_caller_without_the_right_is_refused_in_the_shape_kafka_answers() {
    let rig = rig_with(closed_world);
    rig.scram_alter(vec![], vec![upsert("amy", SHA256, 4096, "p")])
        .await;
    let nobody = user("mallory");
    // Alter: ClusterAuthorizationException through `handleError`, so `getErrorResponse` gives each distinct user
    // name, sorted, the code and no message here (SEC-D8) (AlterUserScramCredentialsRequest.java:67-83).
    let r = rig
        .scram_alter_as(
            &nobody,
            vec![delete("zed", SHA256), delete("amy", SHA256)],
            vec![
                upsert("amy", SHA512, 4096, "p"),
                upsert("bob", SHA256, 4096, "p"),
            ],
        )
        .await;
    let denied = |u: &str| (u.to_owned(), 31, None);
    assert_eq!(r, vec![denied("amy"), denied("bob"), denied("zed")]);
    assert!(rig.scram_alter_as(&nobody, vec![], vec![]).await.is_empty());
    // Describe: the whole request, and one entry per name asked for (DescribeUserScramCredentialsRequest.java:63-76).
    let d = rig
        .scram_describe_as(&nobody, Some(&["amy", "ghost"]))
        .await;
    assert_eq!((d.error_code, d.error_message.clone()), (31, None));
    assert_eq!(d.results.len(), 2);
    assert!(
        d.results
            .iter()
            .all(|x| x.error_code == 31 && x.credential_infos.is_empty())
    );
    assert_eq!(rig.scram_describe_as(&nobody, None).await.error_code, 31);
    // Nothing was changed, and the super user sees it.
    assert_eq!(
        rig.scram_listed().await,
        vec![("amy".to_owned(), vec![(SHA256, 4096)])]
    );
}

#[tokio::test]
async fn alter_needs_alter_on_the_cluster_and_describe_needs_describe_which_alter_implies() {
    let rig = rig_with(closed_world);
    let grant = |who: &str, op| creation((CLUSTER, "kafka-cluster", LITERAL), who, op, ALLOW);
    let made = rig
        .create_acls_as(
            &admin(),
            3,
            vec![grant("User:ops", ALTER), grant("User:reader", DESCRIBE)],
        )
        .await;
    assert!(made.iter().all(|r| r.error_code == 0));
    let (ops, reader) = (user("ops"), user("reader"));
    assert_eq!(
        rig.scram_alter_as(&ops, vec![], vec![upsert("x", SHA256, 4096, "p")])
            .await,
        vec![ok("x")]
    );
    assert_eq!(rig.scram_describe_as(&ops, None).await.results.len(), 1);
    assert_eq!(rig.scram_describe_as(&reader, None).await.results.len(), 1);
    let r = rig
        .scram_alter_as(&reader, vec![delete("x", SHA256)], vec![])
        .await;
    assert_eq!(r, vec![("x".to_owned(), 31, None)]);
    assert_eq!(rig.scram_listed().await.len(), 1);
}

#[tokio::test]
async fn a_user_configured_at_start_is_listed_and_cannot_be_changed_over_the_api() {
    // SEC-D50: the bootstrap entry wins, is never written to the table, and an alteration aimed at it is refused with
    // UNACCEPTABLE_CREDENTIAL (93), the code Kafka uses for a credential it will not take.
    let users = bootstrap_users(&[("admin", "admin-secret")]);
    let rig = rig_with(|c| c.users = Arc::clone(&users));
    let said = "The user is configured at start (--sasl-user, --sasl-users-file or DOZE_KAFKA_SASL_USERS) and cannot be changed over the API";
    let r = rig
        .scram_alter(
            vec![delete("admin", SHA256)],
            vec![upsert("admin", SHA512, 4096, "taken-over")],
        )
        .await;
    // The deletion is judged first and fails, and then every later operation on the name is ignored (:187, :207).
    assert_eq!(r, vec![err("admin", 93, said)]);
    for (deletions, upsertions) in [
        (vec![delete("admin", SHA256)], vec![]),
        (vec![], vec![upsert("admin", SHA512, 4096, "taken-over")]),
        (vec![], vec![upsert("admin", SHA256, 9999, "taken-over")]),
    ] {
        let r = rig.scram_alter(deletions, upsertions).await;
        assert_eq!(r, vec![err("admin", 93, said)]);
    }
    assert!(logs_in(&users, SHA256, "admin", "admin-secret"));
    assert!(!logs_in(&users, SHA256, "admin", "taken-over"));
    assert!(!logs_in(&users, SHA512, "admin", "taken-over"));
    // Describe lists it, with what was derived at start (4096 rounds, for each mechanism that is on).
    assert_eq!(
        rig.scram_listed().await,
        vec![("admin".to_owned(), vec![(SHA256, 4096), (SHA512, 4096)])]
    );
    // Nothing reached the table.
    let again = open_on_with(rig.fs.process_restart(), |_| {});
    assert!(again.scram_listed().await.is_empty());
}

#[tokio::test]
async fn stored_users_come_back_after_a_restart_and_log_in() {
    let rig = rig_with(|_| {});
    rig.scram_alter(vec![], vec![upsert("bob", SHA256, 4096, "bob-secret")])
        .await;
    rig.scram_alter(vec![], vec![upsert("bob", SHA512, 8000, "bob-secret")])
        .await;
    rig.scram_alter(vec![], vec![upsert("gone", SHA256, 4096, "x")])
        .await;
    rig.scram_alter(vec![delete("gone", SHA256)], vec![]).await;
    let users = bootstrap_users(&[]);
    let again = open_on_with(rig.fs.process_restart(), |c| c.users = Arc::clone(&users));
    assert!(logs_in(&users, SHA256, "bob", "bob-secret"));
    assert!(logs_in(&users, SHA512, "bob", "bob-secret"));
    assert!(!logs_in(&users, SHA512, "bob", "wrong"));
    assert!(!logs_in(&users, SHA256, "gone", "x"));
    assert_eq!(
        again.scram_listed().await,
        vec![("bob".to_owned(), vec![(SHA256, 4096), (SHA512, 8000)])]
    );
}

#[tokio::test]
async fn a_name_stored_and_then_configured_at_start_is_the_configured_one_and_the_stored_row_stays()
{
    let rig = rig_with(|_| {});
    rig.scram_alter(vec![], vec![upsert("alice", SHA256, 9000, "stored-secret")])
        .await;
    let users = bootstrap_users(&[("alice", "alice-secret")]);
    let again = open_on_with(rig.fs.process_restart(), |c| c.users = Arc::clone(&users));
    assert!(logs_in(&users, SHA256, "alice", "alice-secret"));
    assert!(!logs_in(&users, SHA256, "alice", "stored-secret"));
    // Described as the configured user (4096, both mechanisms), not the stored one.
    assert_eq!(
        again.scram_listed().await,
        vec![("alice".to_owned(), vec![(SHA256, 4096), (SHA512, 4096)])]
    );
    // Start without the flag again and the stored credential is back, as it was.
    let users = bootstrap_users(&[]);
    open_on_with(again.fs.process_restart(), |c| c.users = Arc::clone(&users));
    assert!(logs_in(&users, SHA256, "alice", "stored-secret"));
}

#[tokio::test]
async fn a_write_that_fails_changes_nothing_and_says_so() {
    let users = bootstrap_users(&[]);
    let rig = rig_with(|c| c.users = Arc::clone(&users));
    rig.scram_alter(vec![], vec![upsert("keep", SHA256, 4096, "p")])
        .await;
    rig.fs.fail_any(1, Failure::Io);
    let r = rig
        .scram_alter(
            vec![delete("keep", SHA256)],
            vec![
                upsert("new", SHA256, 4096, "p"),
                upsert("bad", SHA256, 3, "p"),
            ],
        )
        .await;
    // KAFKA_STORAGE_ERROR (56) for what would have succeeded; the invalid one keeps its own answer.
    assert_eq!(
        r,
        vec![
            ("keep".to_owned(), 56, None),
            ("new".to_owned(), 56, None),
            err("bad", 93, "Too few iterations"),
        ]
    );
    // Neither the table nor the logins changed.
    assert!(logs_in(&users, SHA256, "keep", "p"));
    assert!(!logs_in(&users, SHA256, "new", "p"));
    assert_eq!(
        rig.scram_listed().await,
        vec![("keep".to_owned(), vec![(SHA256, 4096)])]
    );
    // And the table, read after a restart, has the same.
    let again = open_on_with(rig.fs.process_restart(), |_| {});
    assert_eq!(
        again.scram_listed().await,
        vec![("keep".to_owned(), vec![(SHA256, 4096)])]
    );
}
