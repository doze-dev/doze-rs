//! AlterUserScramCredentials and the super users (SEC-190, SEC-D70): on a broker that allows everything by default, any
//! logged-in user holds ALTER on the cluster, so without this rule one could replace or delete the credential of a
//! super user and log in as it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::sync::Arc;

use support::acls::user;
use support::rig_with;
use support::users::{Line, SHA256, SHA512, bootstrap_users, delete, logs_in, upsert};

fn ok(user: &str) -> Line {
    (user.to_owned(), 0, None)
}

fn refused(user: &str) -> Line {
    (user.to_owned(), 31, None)
}

/// An open broker (every logged-in user may do everything) whose super users are `root` and `boss`.
fn open_with_supers(c: &mut kafka_broker::Config) {
    c.super_users.insert("User:root".to_owned());
    c.super_users.insert("User:boss".to_owned());
}

#[tokio::test]
async fn a_user_who_is_not_a_super_user_cannot_set_the_credential_of_one() {
    let users = bootstrap_users(&[]);
    let rig = rig_with(|c| {
        open_with_supers(c);
        c.users = Arc::clone(&users);
    });
    let mallory = user("mallory");
    let r = rig
        .scram_alter_as(&mallory, vec![], vec![upsert("root", SHA256, 4096, "mine")])
        .await;
    assert_eq!(r, vec![refused("root")]);
    assert!(!logs_in(&users, SHA256, "root", "mine"));
    assert!(rig.scram_listed().await.is_empty());
}

#[tokio::test]
async fn a_user_who_is_not_a_super_user_cannot_replace_or_delete_the_stored_credential_of_one() {
    let users = bootstrap_users(&[]);
    let rig = rig_with(|c| {
        open_with_supers(c);
        c.users = Arc::clone(&users);
    });
    let root = user("root");
    let r = rig
        .scram_alter_as(
            &root,
            vec![],
            vec![upsert("boss", SHA256, 4096, "boss-secret")],
        )
        .await;
    assert_eq!(r, vec![ok("boss")]);
    let mallory = user("mallory");
    for (deletions, upsertions) in [
        (vec![delete("boss", SHA256)], vec![]),
        (vec![], vec![upsert("boss", SHA256, 4096, "taken")]),
        (vec![], vec![upsert("boss", SHA512, 4096, "taken")]),
    ] {
        let r = rig.scram_alter_as(&mallory, deletions, upsertions).await;
        assert_eq!(r, vec![refused("boss")]);
    }
    assert!(logs_in(&users, SHA256, "boss", "boss-secret"));
    assert!(!logs_in(&users, SHA256, "boss", "taken"));
    assert!(!logs_in(&users, SHA512, "boss", "taken"));
}

#[tokio::test]
async fn a_request_that_names_a_super_user_and_others_is_refused_for_the_super_user_alone() {
    let rig = rig_with(open_with_supers);
    let mallory = user("mallory");
    let r = rig
        .scram_alter_as(
            &mallory,
            vec![],
            vec![
                upsert("root", SHA256, 4096, "x"),
                upsert("carol", SHA256, 4096, "c"),
            ],
        )
        .await;
    // What happened first, then the refusals (the order of ScramControlManager).
    assert_eq!(r, vec![ok("carol"), refused("root")]);
    assert_eq!(
        rig.scram_listed().await,
        vec![("carol".to_owned(), vec![(SHA256, 4096)])]
    );
}

#[tokio::test]
async fn a_super_user_may_still_manage_anyone_including_another_super_user() {
    let users = bootstrap_users(&[]);
    let rig = rig_with(|c| {
        open_with_supers(c);
        c.users = Arc::clone(&users);
    });
    let root = user("root");
    let r = rig
        .scram_alter_as(
            &root,
            vec![],
            vec![
                upsert("boss", SHA256, 4096, "one"),
                upsert("dave", SHA256, 4096, "d"),
            ],
        )
        .await;
    assert_eq!(r, vec![ok("boss"), ok("dave")]);
    let r = rig
        .scram_alter_as(&root, vec![], vec![upsert("boss", SHA256, 4096, "two")])
        .await;
    assert_eq!(r, vec![ok("boss")]);
    assert!(logs_in(&users, SHA256, "boss", "two"));
    let r = rig
        .scram_alter_as(&root, vec![delete("boss", SHA256)], vec![])
        .await;
    assert_eq!(r, vec![ok("boss")]);
    assert!(!logs_in(&users, SHA256, "boss", "two"));
}

#[tokio::test]
async fn others_are_still_managed_by_anyone_and_a_broker_without_super_users_is_unchanged()
 {
    let rig = rig_with(open_with_supers);
    let r = user("mallory");
    let said = rig
        .scram_alter_as(&r, vec![], vec![upsert("erin", SHA256, 4096, "e")])
        .await;
    assert_eq!(said, vec![ok("erin")]);
    let plain = rig_with(|_| {});
    let said = plain
        .scram_alter_as(
            &user("mallory"),
            vec![],
            vec![upsert("root", SHA256, 4096, "e")],
        )
        .await;
    assert_eq!(said, vec![ok("root")]);
}

#[tokio::test]
async fn a_super_user_configured_at_start_stays_refused_to_everyone() {
    // SEC-D50 first: the bootstrap rule is judged before the super-user rule, and it holds for super users too.
    let users = bootstrap_users(&[("root", "root-secret")]);
    let rig = rig_with(|c| {
        open_with_supers(c);
        c.users = Arc::clone(&users);
    });
    for who in ["mallory", "root"] {
        let r = rig
            .scram_alter_as(&user(who), vec![], vec![upsert("root", SHA256, 4096, "x")])
            .await;
        assert_eq!(r.len(), 1, "{who}");
        assert_eq!(r[0].1, 93, "{who}");
    }
    assert!(logs_in(&users, SHA256, "root", "root-secret"));
}
