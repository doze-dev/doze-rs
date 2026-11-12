//! The three ACL APIs, part three: who may use them, ANONYMOUS without SASL, restarts and concurrency.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::sync::Arc;

use kafka_broker::{Caller, Outcome};
use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use support::acls::{
    ALL, ALLOW, ALTER, CLUSTER, DENY, DESCRIBE, LITERAL, READ, TOPIC, admin, closed_world,
    creation, describe_request, filter, user,
};
use support::{open_on_with, rig_with};

fn text(s: &str) -> Option<StrBytes> {
    Some(StrBytes::from_string(s.to_owned()))
}

/// The admin grants `principal` `op` on the cluster (`perm`: allow or deny).
async fn on_the_cluster(rig: &support::Rig, principal: &str, op: i8, perm: i8) {
    let acl = creation((CLUSTER, "kafka-cluster", LITERAL), principal, op, perm);
    assert_eq!(
        rig.create_acls_as(&admin(), 3, vec![acl]).await[0].error_code,
        0
    );
}

fn one() -> kafka_wire::messages::create_acls_request::AclCreation {
    creation((TOPIC, "t", LITERAL), "User:alice", READ, ALLOW)
}

fn codes<T>(results: &[T], code: impl Fn(&T) -> i16) -> Vec<i16> {
    results.iter().map(code).collect()
}

#[tokio::test]
async fn a_caller_with_nothing_on_the_cluster_is_refused_with_31_everywhere() {
    let rig = rig_with(closed_world);
    let alice = user("alice");
    let r = rig.create_acls_as(&alice, 3, vec![one(), one()]).await;
    assert_eq!(codes(&r, |x| x.error_code), [31, 31]);
    let r = rig
        .delete_acls_as(&alice, 3, vec![filter(1, 1, 1, 1), filter(1, 1, 1, 1)])
        .await;
    assert_eq!(codes(&r, |x| x.error_code), [31, 31]);
    let d = rig.describe_acls_as(&alice, 3, &filter(1, 1, 1, 1)).await;
    assert_eq!(
        (d.error_code, d.error_message, d.resources.len()),
        (31, text(""), 0)
    );
    // The unknown-code close comes before the authorization: a caller with nothing is closed too.
    let o = rig
        .outcome_as(
            &alice,
            3,
            Request::DescribeAcls(describe_request(&filter(0, 1, 1, 1))),
        )
        .await;
    assert!(matches!(o, Outcome::Close));
    // ... and a request Kafka cannot build (ANY) is answered 31 to a caller who may not, not -1.
    let mut any = one();
    any.operation = 1;
    assert_eq!(
        rig.create_acls_as(&alice, 3, vec![any]).await[0].error_code,
        31
    );
}

#[tokio::test]
async fn describe_and_alter_on_the_cluster_open_the_apis_one_by_one_and_a_deny_closes_them() {
    let rig = rig_with(closed_world);
    let (alice, bob) = (user("alice"), user("bob"));
    // DESCRIBE alone lets alice describe, not alter.
    on_the_cluster(&rig, "User:alice", DESCRIBE, ALLOW).await;
    assert_eq!(
        rig.describe_acls_as(&alice, 3, &filter(1, 1, 1, 1))
            .await
            .error_code,
        0
    );
    assert_eq!(
        rig.create_acls_as(&alice, 3, vec![one()]).await[0].error_code,
        31
    );
    // ALTER allows create and delete, and (by implication) describe.
    on_the_cluster(&rig, "User:bob", ALTER, ALLOW).await;
    assert_eq!(
        rig.create_acls_as(&bob, 3, vec![one()]).await[0].error_code,
        0
    );
    assert_eq!(
        rig.describe_acls_as(&bob, 3, &filter(1, 1, 1, 1))
            .await
            .error_code,
        0
    );
    let r = rig
        .delete_acls_as(&bob, 3, vec![filter(TOPIC, 1, 1, 1)])
        .await;
    assert_eq!((r[0].error_code, r[0].matching_acls.len()), (0, 1));
    // A DENY of everything on the cluster takes it away again, even from the one who has the ALLOW ...
    on_the_cluster(&rig, "User:bob", ALL, DENY).await;
    assert_eq!(
        rig.create_acls_as(&bob, 3, vec![one()]).await[0].error_code,
        31
    );
    // ... but a super user cannot be locked out.
    assert_eq!(
        rig.create_acls_as(&admin(), 3, vec![one()]).await[0].error_code,
        0
    );
}

#[tokio::test]
async fn with_no_sasl_the_anonymous_principal_is_checked_like_any_other() {
    // The default allows, nothing is stored: the anonymous caller may use the APIs, and what it stores binds it.
    let rig = rig_with(|_| {});
    let nobody = Caller::anonymous(None);
    let r = rig
        .create_acls_as(
            &nobody,
            3,
            vec![creation(
                (TOPIC, "t", LITERAL),
                "User:ANONYMOUS",
                READ,
                ALLOW,
            )],
        )
        .await;
    assert_eq!(r[0].error_code, 0);
    // A topic ACL does not cover the cluster: still allowed by the default.
    assert_eq!(
        rig.describe_acls_as(&nobody, 3, &filter(1, 1, 1, 1))
            .await
            .resources
            .len(),
        1
    );
    // A cluster ACL for somebody else turns the default off for it.
    rig.create_acls_as(
        &nobody,
        3,
        vec![creation(
            (CLUSTER, "kafka-cluster", LITERAL),
            "User:other",
            ALTER,
            ALLOW,
        )],
    )
    .await;
    assert_eq!(
        rig.describe_acls_as(&nobody, 3, &filter(1, 1, 1, 1))
            .await
            .error_code,
        31
    );
}

#[tokio::test]
async fn acls_outlive_a_restart_and_still_decide() {
    let rig = rig_with(closed_world);
    rig.create_acls_as(
        &admin(),
        3,
        vec![
            creation(
                (CLUSTER, "kafka-cluster", LITERAL),
                "User:alice",
                DESCRIBE,
                ALLOW,
            ),
            creation((TOPIC, "gone", LITERAL), "User:alice", READ, ALLOW),
        ],
    )
    .await;
    let mut f = filter(TOPIC, 1, 1, 1);
    f.resource_name_filter = text("gone");
    rig.delete_acls_as(&admin(), 3, vec![f]).await;
    let again = open_on_with(rig.fs.process_restart(), closed_world);
    assert_eq!(again.acls_as(&admin()).await.len(), 1);
    // The ACL that came back is enforced: alice may describe the ACLs, and bob may not.
    assert_eq!(
        again
            .describe_acls_as(&user("alice"), 3, &filter(1, 1, 1, 1))
            .await
            .error_code,
        0
    );
    assert_eq!(
        again
            .describe_acls_as(&user("bob"), 3, &filter(1, 1, 1, 1))
            .await
            .error_code,
        31
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn changes_at_once_lose_nothing() {
    let rig = Arc::new(rig_with(closed_world));
    let tasks: Vec<_> = (0..8)
        .map(|t| {
            let rig = Arc::clone(&rig);
            tokio::spawn(async move {
                for i in 0..6 {
                    let name = format!("t{t}-{i}");
                    let r = rig
                        .create_acls_as(
                            &admin(),
                            3,
                            vec![creation((TOPIC, &name, LITERAL), "User:x", READ, ALLOW)],
                        )
                        .await;
                    assert_eq!(r[0].error_code, 0);
                    if i % 3 == 2 {
                        // Delete the one before: deletes and creates run together too.
                        let mut f = filter(TOPIC, 1, 1, 1);
                        f.resource_name_filter = text(&format!("t{t}-{}", i - 1));
                        let r = rig.delete_acls_as(&admin(), 3, vec![f]).await;
                        assert_eq!(r[0].matching_acls.len(), 1);
                    }
                }
            })
        })
        .collect();
    for t in tasks {
        t.await.unwrap();
    }
    // 8 tasks x (6 created - 2 deleted).
    let all = rig.acls_as(&admin()).await;
    assert_eq!(all.len(), 8 * 4);
    // What is in force is what is stored: the same after a restart.
    let again = open_on_with(rig.fs.process_restart(), closed_world);
    assert_eq!(again.acls_as(&admin()).await, all);
}
