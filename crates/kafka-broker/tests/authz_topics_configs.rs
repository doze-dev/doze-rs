//! DescribeConfigs, AlterConfigs and IncrementalAlterConfigs under ACLs (slice 8d-1). Every row is from the Kafka 4.3.1
//! source, not from a recording: `ConfigHelper.scala` `handleDescribeConfigsRequest` (49-80),
//! `ConfigAdminManager.scala` `preprocess` (100-160, 209-261), `ControllerApis.scala` `authorizeAlterResource` (472-495),
//! `handleLegacyAlterConfigs` (497-554) and `handleIncrementalAlterConfigs` (715-794).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use support::TOPIC_RESOURCE;
use support::acls::{ALLOW, CLUSTER, DESCRIBE, GROUP, LITERAL, creation, user};
use topics::*;

const BROKER: i8 = 4;
const BROKER_LOGGER: i8 = 8;
const CLIENT_METRICS: i8 = 16;
const GROUP_RESOURCE: i8 = 32;
const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const INVALID_REQUEST: i16 = 42;

fn cluster(principal: &str, op: i8) -> kafka_wire::messages::create_acls_request::AclCreation {
    creation((CLUSTER, "kafka-cluster", LITERAL), principal, op, ALLOW)
}

fn on_group(
    name: &str,
    principal: &str,
    op: i8,
) -> kafka_wire::messages::create_acls_request::AclCreation {
    creation((GROUP, name, LITERAL), principal, op, ALLOW)
}

// DescribeConfigs

#[tokio::test]
async fn describe_configs_answers_the_refused_resources_after_the_others_with_no_configs_and_the_errors_words()
 {
    let rig = rig_with_topics(&["t", "other"]).await;
    grant(
        &rig,
        vec![
            allow("t", "User:alice", DESCRIBE_CONFIGS),
            on_group("g", "User:alice", DESCRIBE_CONFIGS),
        ],
    )
    .await;
    for version in 1..=4 {
        let r = rig
            .describe_configs_as(
                &user("alice"),
                version,
                &[
                    (BROKER, ""),
                    (TOPIC_RESOURCE, "t"),
                    (TOPIC_RESOURCE, "other"),
                    (GROUP_RESOURCE, "g"),
                    (GROUP_RESOURCE, "h"),
                    (BROKER_LOGGER, "0"),
                    (CLIENT_METRICS, "m"),
                ],
            )
            .await;
        let shape: Vec<_> = r
            .iter()
            .map(|x| (x.resource_type, x.resource_name.to_string(), x.error_code))
            .collect();
        // Authorized first, in the order asked: only the topic (the group is a resource type this broker does not
        // take, and is answered as such, not refused); then those refused.
        assert_eq!(
            shape,
            [
                (TOPIC_RESOURCE, "t".into(), 0),
                (GROUP_RESOURCE, "g".into(), 42),
                (BROKER, "".into(), 31),
                (TOPIC_RESOURCE, "other".into(), 29),
                (GROUP_RESOURCE, "h".into(), 30),
                (BROKER_LOGGER, "0".into(), 31),
                (CLIENT_METRICS, "m".into(), 31),
            ],
            "v{version}"
        );
        let refused = &r[2];
        assert!(refused.configs.is_empty());
        assert_eq!(
            refused.error_message.as_deref(),
            Some("Cluster authorization failed.")
        );
        assert_eq!(
            r[3].error_message.as_deref(),
            Some("Topic authorization failed.")
        );
        assert_eq!(
            r[4].error_message.as_deref(),
            Some("Group authorization failed.")
        );
    }
}

#[tokio::test]
async fn describe_configs_is_granted_by_alter_configs_and_a_topic_that_is_not_there_is_hidden() {
    let rig = rig_with_topics(&["t"]).await;
    grant(
        &rig,
        vec![
            allow("t", "User:alice", ALTER_CONFIGS),
            allow("t", "User:bob", DESCRIBE),
            allow("ghost", "User:alice", ALTER_CONFIGS),
            cluster("User:ops", DESCRIBE_CONFIGS),
        ],
    )
    .await;
    let code = |who: &'static str, kind: i8, name: &'static str| {
        let rig = &rig;
        async move {
            rig.describe_configs_as(&user(who), 4, &[(kind, name)])
                .await[0]
                .error_code
        }
    };
    assert_eq!(
        code("alice", TOPIC_RESOURCE, "t").await,
        0,
        "ALTER_CONFIGS implies DESCRIBE_CONFIGS"
    );
    assert_eq!(
        code("bob", TOPIC_RESOURCE, "t").await,
        29,
        "DESCRIBE does not"
    );
    assert_eq!(
        code("alice", TOPIC_RESOURCE, "ghost").await,
        UNKNOWN_TOPIC_OR_PARTITION
    );
    assert_eq!(code("bob", TOPIC_RESOURCE, "ghost").await, 29);
    // A broker's configs are the cluster's to describe.
    assert_eq!(code("ops", BROKER, "").await, 0);
    assert_eq!(code("ops", BROKER, "1").await, 0);
    assert_eq!(code("alice", BROKER, "").await, 31);
    // The cluster right does not give a topic's configs.
    assert_eq!(code("ops", TOPIC_RESOURCE, "t").await, 29);
}

#[tokio::test]
async fn describe_configs_leaves_an_unknown_resource_type_to_the_request_and_not_to_the_acls() {
    let rig = rig_with_topics(&["t"]).await;
    let r = rig
        .describe_configs_as(&user("nobody"), 4, &[(99, "x")])
        .await;
    assert_eq!(r[0].error_code, INVALID_REQUEST);
}
