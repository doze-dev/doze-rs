//! Authorization of the streams group APIs (KIP-1071), SEC-330 to SEC-333. No row is recorded: each test cites the Kafka 4.4.0
//! source it is written from (`core/src/main/scala/kafka/server/KafkaApis.scala`, "KA" below), and the checks of one right
//! are put five ways (`five_ways!`: the right granted, missing, DENY against a super user and against ALLOW, the broad
//! grants).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[macro_use]
#[path = "support/authz_groups.rs"]
mod gx;

use gx::{CLUSTER, DESCRIBE, GROUP, GROUP_AUTH, READ, TOPIC, TOPIC_AUTH, allow, grant, make_topic, need, user, world};
use kafka_broker::Caller;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::StreamsGroupDescribeRequest;
use support::acls::admin;
use support::streams::join;

const CREATE: i8 = 5;

/// Whether the topic exists, as the administrator sees it.
async fn exists(rig: &support::Rig, name: &str) -> bool {
    rig.metadata_as(&admin(), 12, Some(&[name]), false).await.topics[0].error_code == 0
}

fn member_of(who: &Caller) -> String {
    format!("m-{}", who.principal().as_str().replace(':', "-"))
}

// ---- StreamsGroupHeartbeat (KA L2809-2861: READ on the group, then DESCRIBE on every topic the topology names; the code
// alone)

#[tokio::test]
async fn the_streams_heartbeat_needs_read_on_the_group_and_describe_on_each_topic_of_the_topology() {
    five_ways!(
        &[
            need(GROUP, "grp", READ, GROUP_AUTH),
            need(TOPIC, "tpc", DESCRIBE, TOPIC_AUTH),
            need(TOPIC, "grp-words-repartition", DESCRIBE, TOPIC_AUTH),
            need(TOPIC, "grp-counts-changelog", DESCRIBE, TOPIC_AUTH)
        ],
        |rig, who| {
            make_topic(rig, "tpc", 1).await;
            let req = join("grp", &member_of(who), "tpc");
            match rig.ask_as(who, 1, Request::StreamsGroupHeartbeat(req.into())).await {
                Response::StreamsGroupHeartbeat(r) => {
                    if r.error_code == GROUP_AUTH || r.error_code == TOPIC_AUTH {
                        assert_eq!(r.error_message, None, "the code alone");
                    }
                    r.error_code
                }
                other => panic!("{other:?}"),
            }
        }
    );
}

// ---- StreamsGroupDescribe (KA L2968-3066: DESCRIBE on each group)

#[tokio::test]
async fn streams_group_describe_needs_describe_on_the_group() {
    five_ways!(&[need(GROUP, "grp", DESCRIBE, GROUP_AUTH)], |rig, who| {
        let mut req = StreamsGroupDescribeRequest::default();
        req.group_ids = vec![gx::group_id("grp")];
        match rig.ask_as(who, 1, Request::StreamsGroupDescribe(req)).await {
            Response::StreamsGroupDescribe(r) => r.groups[0].error_code,
            other => panic!("{other:?}"),
        }
    });
}

/// KA L3018-3060: a group whose topology names a topic the caller may not DESCRIBE is told 29, with Kafka's words, and
/// nothing else of it.
#[tokio::test]
async fn a_group_whose_topology_names_a_topic_the_caller_may_not_describe_is_hidden() {
    let rig = world();
    make_topic(&rig, "tpc", 1).await;
    let made = rig.ask_as(&admin(), 1, Request::StreamsGroupHeartbeat(join("grp", "m", "tpc").into())).await;
    assert!(matches!(made, Response::StreamsGroupHeartbeat(ref r) if r.error_code == 0), "{made:?}");
    let reader = user("reader");
    grant(&rig, vec![allow(GROUP, "grp", "User:reader", DESCRIBE), allow(TOPIC, "tpc", "User:reader", DESCRIBE)]).await;
    let mut req = StreamsGroupDescribeRequest::default();
    req.group_ids = vec![gx::group_id("grp")];
    let Response::StreamsGroupDescribe(r) = rig.ask_as(&reader, 1, Request::StreamsGroupDescribe(req)).await else {
        panic!()
    };
    let g = &r.groups[0];
    assert_eq!(g.error_code, TOPIC_AUTH);
    assert_eq!(
        g.error_message.as_ref().map(ToString::to_string),
        Some("The described group uses topics that the client is not authorized to describe.".into())
    );
    assert!(g.topology.is_none() && g.members.is_empty());
}

/// KA L2873-2896: the internal topics are made only if the caller may CREATE them (on the cluster, or each topic); if it may
/// not make one, none is made and the MISSING_INTERNAL_TOPICS status says which.
#[tokio::test]
async fn internal_topics_are_made_only_by_a_caller_that_may_create_them() {
    let rig = world();
    make_topic(&rig, "tpc", 1).await;
    let app = user("app");
    let mut rights = vec![allow(GROUP, "grp", "User:app", READ)];
    for t in ["tpc", "grp-words-repartition", "grp-counts-changelog"] {
        rights.push(allow(TOPIC, t, "User:app", DESCRIBE));
    }
    rights.push(allow(TOPIC, "grp-words-repartition", "User:app", CREATE));
    grant(&rig, rights).await;
    let Response::StreamsGroupHeartbeat(r) = rig.ask_as(&app, 1, Request::StreamsGroupHeartbeat(join("grp", "m", "tpc").into())).await
    else {
        panic!()
    };
    assert_eq!(r.error_code, 0);
    let detail = r.status.unwrap().into_iter().find(|s| s.status_code == 3).unwrap().status_detail.to_string();
    assert!(detail.ends_with("; Unauthorized to CREATE on topics grp-counts-changelog."), "{detail}");
    assert!(!exists(&rig, "grp-words-repartition").await, "none is made");
    // CREATE on the cluster covers them all.
    grant(&rig, vec![allow(CLUSTER, "kafka-cluster", "User:app", CREATE)]).await;
    let Response::StreamsGroupHeartbeat(again) = rig.ask_as(&app, 1, Request::StreamsGroupHeartbeat(join("grp", "m", "tpc").into())).await
    else {
        panic!()
    };
    assert_eq!(again.error_code, 0);
    assert!(exists(&rig, "grp-words-repartition").await && exists(&rig, "grp-counts-changelog").await);
}
