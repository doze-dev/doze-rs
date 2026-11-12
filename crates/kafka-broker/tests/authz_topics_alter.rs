//! AlterConfigs and IncrementalAlterConfigs under ACLs (slice 8d-1). Every row is from the Kafka 4.3.1 source, not from a
//! recording: `ConfigAdminManager.scala` `preprocess` (100-160, 209-261), `ControllerApis.scala` `authorizeAlterResource`
//! (472-495), `handleLegacyAlterConfigs` (497-554) and `handleIncrementalAlterConfigs` (715-794).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::IncrementalAlterConfigsRequest;
use kafka_wire::messages::incremental_alter_configs_request::{
    AlterConfigsResource as Resource, AlterableConfig as Config,
};
use support::acls::{
    ALLOW, ALTER, CLUSTER, DENY, DESCRIBE, GROUP, LITERAL, READ, admin, creation, user,
};
use support::{Rig, TOPIC_RESOURCE};
use topics::*;

const BROKER: i8 = 4;
const BROKER_LOGGER: i8 = 8;
const CLIENT_METRICS: i8 = 16;
const GROUP_RESOURCE: i8 = 32;
const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const INVALID_REQUEST: i16 = 42;
const GROUP_AUTHORIZATION_FAILED: i16 = 30;

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

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

// AlterConfigs and IncrementalAlterConfigs

async fn both(rig: &Rig, who: &str, resources: &[(i8, &str)]) -> [Vec<(i16, Option<String>)>; 2] {
    [
        rig.alter_as(&user(who), resources).await,
        rig.incremental_as(&user(who), resources).await,
    ]
}

#[tokio::test]
async fn a_resource_the_caller_may_not_alter_is_refused_with_that_resources_error_and_words() {
    let rig = rig_with_topics(&["t", "u"]).await;
    grant(
        &rig,
        vec![
            allow("t", "User:alice", ALTER_CONFIGS),
            cluster("User:ops", ALTER_CONFIGS),
            on_group("g", "User:gina", ALTER_CONFIGS),
        ],
    )
    .await;
    // (the AlterConfigs and IncrementalAlterConfigs answers are the same: one per resource, in the order asked)
    let asked = [
        (TOPIC_RESOURCE, "t"),
        (TOPIC_RESOURCE, "u"),
        (BROKER, ""),
        (GROUP_RESOURCE, "g"),
    ];
    for answers in both(&rig, "alice", &asked).await {
        assert_eq!(answers[0].0, 0, "alice may alter t");
        assert_eq!(answers[1], (29, Some("Topic authorization failed.".into())));
        assert_eq!(
            answers[2],
            (31, Some("Cluster authorization failed.".into()))
        );
        assert_eq!(
            answers[3],
            (
                GROUP_AUTHORIZATION_FAILED,
                Some("Group authorization failed.".into())
            )
        );
    }
    for answers in both(
        &rig,
        "ops",
        &[(BROKER, ""), (CLIENT_METRICS, "m"), (TOPIC_RESOURCE, "t")],
    )
    .await
    {
        assert_eq!(
            answers[0].0, 0,
            "ALTER_CONFIGS on the cluster is what a broker's configs need"
        );
        assert_eq!(
            answers[1].0, 42,
            "client metrics are the cluster's, and not taken here: past authorization"
        );
        assert_eq!(answers[2].0, 29);
    }
    for answers in both(
        &rig,
        "gina",
        &[(GROUP_RESOURCE, "g"), (GROUP_RESOURCE, "h")],
    )
    .await
    {
        assert_eq!(
            answers[0].0, 42,
            "the group is allowed through and is a type this broker does not take"
        );
        assert_eq!(answers[1].0, GROUP_AUTHORIZATION_FAILED);
    }
}

#[tokio::test]
async fn the_loggers_of_a_broker_are_the_clusters_in_the_incremental_api_only() {
    let rig = rig_with_topics(&[]).await;
    grant(&rig, vec![cluster("User:ops", ALTER_CONFIGS)]).await;
    // IncrementalAlterConfigs: BROKER_LOGGER needs ALTER_CONFIGS on the cluster (31 without it).
    assert_eq!(
        rig.incremental_as(&user("nobody"), &[(BROKER_LOGGER, "0")])
            .await[0]
            .0,
        31
    );
    assert_ne!(
        rig.incremental_as(&user("ops"), &[(BROKER_LOGGER, "0")])
            .await[0]
            .0,
        31
    );
    // The legacy API does not know the type at all (preprocessing, before authorization): not 31 for anyone.
    assert_ne!(
        rig.alter_as(&user("nobody"), &[(BROKER_LOGGER, "0")]).await[0].0,
        31
    );
}

#[tokio::test]
async fn preprocessing_errors_are_said_before_authorization_is_asked() {
    // ConfigAdminManager.preprocess runs on the broker before the request is forwarded; an unauthorized caller sees these,
    // never a 29 or 31, for the resources they affect.
    let rig = rig_with_topics(&["t"]).await;
    let nobody = user("nobody");

    // A resource twice: both copies, and a third that is single is judged on its own.
    let r = rig
        .incremental_as(
            &nobody,
            &[
                (TOPIC_RESOURCE, "t"),
                (TOPIC_RESOURCE, "t"),
                (TOPIC_RESOURCE, "u"),
            ],
        )
        .await;
    assert_eq!(
        r.iter().map(|x| x.0).collect::<Vec<_>>(),
        [INVALID_REQUEST, INVALID_REQUEST, 29]
    );
    assert_eq!(
        r[0].1.as_deref(),
        Some("Each resource must appear at most once.")
    );
    let r = rig.alter_as(&nobody, &[(BROKER, ""), (BROKER, "")]).await;
    assert_eq!(
        r.iter().map(|x| x.0).collect::<Vec<_>>(),
        [INVALID_REQUEST, INVALID_REQUEST]
    );

    // A config key twice, and a null value, in the resource itself.
    let duplicate_keys = incremental_request(&[(TOPIC_RESOURCE, "t")], |r| {
        r.configs.push(config("retention.ms", 0, Some("1000")));
        r.configs.push(config("retention.ms", 0, Some("2000")));
    });
    let r = rig.incremental_raw(&nobody, duplicate_keys).await;
    assert_eq!(
        r[0],
        (
            INVALID_REQUEST,
            Some("Error due to duplicate config keys".into())
        )
    );
    let null_value = incremental_request(&[(TOPIC_RESOURCE, "t")], |r| {
        r.configs.push(config("retention.ms", 0, None));
        r.configs.push(config("segment.ms", 0, None));
    });
    let r = rig.incremental_raw(&nobody, null_value).await;
    assert_eq!(
        r[0],
        (
            INVALID_REQUEST,
            Some("Null value not supported for : retention.ms, segment.ms".into())
        )
    );

    // A broker named by something that is not its id, or by another id.
    let r = rig
        .incremental_as(&nobody, &[(BROKER, "zero"), (BROKER, "7"), (BROKER, "1")])
        .await;
    assert_eq!(
        r[0],
        (
            INVALID_REQUEST,
            Some("Node id must be an integer, but it is: zero".into())
        )
    );
    assert_eq!(
        r[1],
        (
            INVALID_REQUEST,
            Some("Unexpected broker id, expected 1, but received 7".into())
        )
    );
    assert_eq!(
        r[2].0, 31,
        "a valid broker, by a caller with no right to it"
    );
    // A broker config that does not validate (unknown name) is the same: said before the right is asked.
    let bad = incremental_request(&[(BROKER, "")], |r| {
        r.configs.push(config("no.such.config", 0, Some("1")));
    });
    let r = rig.incremental_raw(&nobody, bad).await;
    assert_eq!(r[0].0, INVALID_REQUEST);
    assert_ne!(r[0].1.as_deref(), Some("Cluster authorization failed."));
}

#[tokio::test]
async fn a_topic_the_caller_may_not_alter_is_29_even_when_it_is_missing_or_named_badly() {
    // Authorization is asked of a topic resource before the topic is looked for or its name judged (the controller does
    // those after `authorizeAlterResource`).
    let rig = rig_with_topics(&["t"]).await;
    grant(
        &rig,
        vec![
            allow("ghost", "User:alice", ALTER_CONFIGS),
            allow("bad name", "User:alice", ALTER_CONFIGS),
        ],
    )
    .await;
    for answers in both(
        &rig,
        "bob",
        &[
            (TOPIC_RESOURCE, "ghost"),
            (TOPIC_RESOURCE, "bad name"),
            (TOPIC_RESOURCE, ""),
        ],
    )
    .await
    {
        assert!(answers.iter().all(|a| a.0 == 29), "{answers:?}");
    }
    for answers in both(
        &rig,
        "alice",
        &[(TOPIC_RESOURCE, "ghost"), (TOPIC_RESOURCE, "bad name")],
    )
    .await
    {
        assert_eq!(answers[0].0, UNKNOWN_TOPIC_OR_PARTITION);
        assert_eq!(answers[1].0, 17);
    }
}

#[tokio::test]
async fn altering_needs_alter_configs_and_nothing_else_grants_it() {
    let rig = rig_with_topics(&["t"]).await;
    grant(
        &rig,
        vec![
            allow("t", "User:reader", READ),
            allow("t", "User:describer", DESCRIBE),
            allow("t", "User:describer", DESCRIBE_CONFIGS),
            allow("t", "User:alterer", ALTER),
            allow("t", "User:alterer", ALTER_CONFIGS),
            deny("t", "User:alterer", ALTER_CONFIGS),
        ],
    )
    .await;
    for who in ["reader", "describer", "alterer"] {
        for answers in both(&rig, who, &[(TOPIC_RESOURCE, "t")]).await {
            assert_eq!(answers[0].0, 29, "{who}");
        }
    }
    let seen = rig
        .describe_configs_as(&admin(), 4, &[(TOPIC_RESOURCE, "t")])
        .await;
    let v = seen[0]
        .configs
        .iter()
        .find(|c| &*c.name == "retention.ms")
        .unwrap();
    assert_ne!(
        v.value.as_deref(),
        Some("5000"),
        "no refused request changed anything"
    );
    // And the caller who has it can: the same request from the super user, with ALTER_CONFIGS given to carol.
    grant(&rig, vec![allow("t", "User:carol", ALTER_CONFIGS)]).await;
    assert_eq!(
        rig.alter_as(&user("carol"), &[(TOPIC_RESOURCE, "t")]).await[0].0,
        0
    );
    let seen = rig
        .describe_configs_as(&admin(), 4, &[(TOPIC_RESOURCE, "t")])
        .await;
    let v = seen[0]
        .configs
        .iter()
        .find(|c| &*c.name == "retention.ms")
        .unwrap();
    assert_eq!(v.value.as_deref(), Some("5000"));
    let _ = DENY;
}

fn config(name: &str, op: i8, value: Option<&str>) -> Config {
    let mut c = Config::default();
    c.name = text(name);
    c.config_operation = op;
    c.value = value.map(text);
    c
}

fn incremental_request(
    resources: &[(i8, &str)],
    fill: impl Fn(&mut Resource),
) -> IncrementalAlterConfigsRequest {
    let mut req = IncrementalAlterConfigsRequest::default();
    for (kind, name) in resources {
        let mut r = Resource::default();
        r.resource_type = *kind;
        r.resource_name = text(name);
        fill(&mut r);
        req.resources.push(r);
    }
    req
}

impl Rig {
    async fn incremental_raw(
        &self,
        who: &kafka_broker::Caller,
        req: IncrementalAlterConfigsRequest,
    ) -> Vec<(i16, Option<String>)> {
        let Response::IncrementalAlterConfigs(r) = self
            .ask_as(who, 1, Request::IncrementalAlterConfigs(req))
            .await
        else {
            panic!("not IncrementalAlterConfigs")
        };
        r.responses
            .into_iter()
            .map(|a| (a.error_code, a.error_message.map(|m| m.to_string())))
            .collect()
    }
}
