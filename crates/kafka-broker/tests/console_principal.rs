//! The console's own principal, `User:__console` (SEC-240 to SEC-243): a super user of every broker, and nobody else can
//! be it: no credential may be made for the name, and a stored one is never put in force.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_auth::Principal;
use kafka_broker::Caller;
use support::acls::{DENY, LITERAL, TOPIC, WRITE, creation, user};
use support::users::{SHA256, delete, upsert};
use support::{rig_with, topic};

fn console() -> Caller {
    Caller::new(Principal::console(), Some("127.0.0.1".parse().unwrap()))
}

#[tokio::test]
async fn the_console_is_a_super_user_where_everyone_else_is_refused() {
    let rig = rig_with(|c| c.acl_default_allow = false);
    let nobody = user("mallory");
    // Everyone else: no ACL, default deny: creating a topic is refused (TOPIC_AUTHORIZATION_FAILED, 29).
    let r = rig
        .ask_as(
            &nobody,
            7,
            kafka_wire::api::Request::CreateTopics(create("made-by-nobody")),
        )
        .await;
    assert_eq!(first_code(&r), 29);
    // The console is a super user: the same request is served.
    let r = rig
        .ask_as(
            &console(),
            7,
            kafka_wire::api::Request::CreateTopics(create("made-by-the-console")),
        )
        .await;
    assert_eq!(first_code(&r), 0);
}

#[tokio::test]
async fn a_deny_acl_does_not_stop_the_console() {
    let rig = rig_with(|c| c.acl_default_allow = true);
    let denied = vec![creation(
        (TOPIC, "t", LITERAL),
        "User:__console",
        WRITE,
        DENY,
    )];
    assert!(
        rig.create_acls_as(&user("x"), 3, denied)
            .await
            .iter()
            .all(|r| r.error_code == 0)
    );
    let r = rig
        .ask_as(
            &console(),
            7,
            kafka_wire::api::Request::CreateTopics(create("t")),
        )
        .await;
    assert_eq!(first_code(&r), 0, "a super user is not refused by a DENY");
}

#[tokio::test]
async fn no_credential_can_be_made_or_removed_for_the_consoles_name() {
    let rig = rig_with(|_| {});
    let admin = user("admin");
    let lines = rig
        .scram_alter_as(
            &admin,
            vec![delete("__console", SHA256)],
            vec![upsert("__console", SHA256, 4096, "x")],
        )
        .await;
    assert_eq!(
        lines.len(),
        1,
        "a name touched twice in one request is one refusal: {lines:?}"
    );
    let lines = rig
        .scram_alter_as(&admin, vec![], vec![upsert("__console", SHA256, 4096, "x")])
        .await;
    assert_eq!(lines[0].0, "__console");
    assert_eq!(lines[0].1, 93, "UNACCEPTABLE_CREDENTIAL");
    assert!(
        lines[0]
            .2
            .as_deref()
            .unwrap()
            .contains("reserved for the console"),
        "{lines:?}"
    );
    let lines = rig
        .scram_alter_as(&admin, vec![delete("__console", SHA256)], vec![])
        .await;
    assert_eq!(lines[0].1, 93);
    // Even the console itself may not give itself a password.
    let lines = rig
        .scram_alter_as(
            &console(),
            vec![],
            vec![upsert("__console", SHA256, 4096, "x")],
        )
        .await;
    assert_eq!(lines[0].1, 93);
    assert!(rig.scram_listed().await.is_empty());
}

#[tokio::test]
async fn a_name_that_only_looks_like_the_consoles_is_an_ordinary_user() {
    let rig = rig_with(|_| {});
    for name in ["__console2", "_console", "__Console", "console"] {
        let lines = rig
            .scram_alter_as(
                &user("admin"),
                vec![],
                vec![upsert(name, SHA256, 4096, "x")],
            )
            .await;
        assert_eq!(lines, vec![(name.to_owned(), 0, None)], "{name}");
    }
}

fn create(name: &str) -> kafka_wire::messages::CreateTopicsRequest {
    let mut r = kafka_wire::messages::CreateTopicsRequest::default();
    r.topics = vec![topic(name, 1)];
    r
}

fn first_code(r: &kafka_wire::api::Response) -> i16 {
    let kafka_wire::api::Response::CreateTopics(r) = r else {
        panic!("not CreateTopics")
    };
    r.topics[0].error_code
}
