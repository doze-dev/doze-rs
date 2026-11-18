//! Authorization of the share group APIs (KIP-932), SEC-320 to SEC-327. No row is recorded: each test cites the Kafka 4.4.0
//! source it is written from (`core/src/main/scala/kafka/server/KafkaApis.scala`, "KA" below), and each is put five ways
//! (`five_ways!`: the right granted, missing, DENY against a super user and against ALLOW, and the broad grants).
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

use gx::{DELETE, DESCRIBE, GROUP, GROUP_AUTH, READ, TOPIC, TOPIC_AUTH, make_topic, need};
use kafka_broker::Caller;
use kafka_wire::Uuid;
use kafka_wire::api::{Request, Response};
use support::Rig;
use support::acls::admin;
use support::share::{ack_request, fetch_request, heartbeat_request};

const ALTER_CONFIGS: i8 = 11;

fn member_of(who: &Caller) -> String {
    format!("m-{}", who.principal().as_str().replace(':', "-"))
}

async fn topic_id(rig: &Rig, name: &str) -> Uuid {
    make_topic(rig, name, 1).await;
    rig.metadata_as(&admin(), 12, Some(&[name]), false)
        .await
        .topics[0]
        .topic_id
}

/// The first error a share answer carries: top-level, else the first partition's.
fn first_code(top: i16, parts: impl IntoIterator<Item = i16>) -> i16 {
    if top != 0 {
        return top;
    }
    parts.into_iter().find(|c| *c != 0).unwrap_or(0)
}

// ---- ShareGroupHeartbeat (KA L3146-3187: READ on the group, DESCRIBE on each subscribed topic; the code alone)

#[tokio::test]
async fn the_share_heartbeat_needs_read_on_the_group_and_describe_on_each_subscribed_topic() {
    five_ways!(
        &[
            need(GROUP, "grp", READ, GROUP_AUTH),
            need(TOPIC, "tpc", DESCRIBE, TOPIC_AUTH)
        ],
        |rig, who| {
            make_topic(rig, "tpc", 1).await;
            let req = heartbeat_request("grp", &member_of(who), 0, Some(&["tpc"]));
            match rig.ask_as(who, 1, Request::ShareGroupHeartbeat(req)).await {
                Response::ShareGroupHeartbeat(r) => {
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

// ---- ShareGroupDescribe (KA L3189-3269: DESCRIBE on each group)

#[tokio::test]
async fn share_group_describe_needs_describe_on_the_group() {
    five_ways!(&[need(GROUP, "grp", DESCRIBE, GROUP_AUTH)], |rig, who| {
        let mut req = kafka_wire::messages::ShareGroupDescribeRequest::default();
        req.group_ids = vec![gx::group_id("grp")];
        match rig.ask_as(who, 1, Request::ShareGroupDescribe(req)).await {
            Response::ShareGroupDescribe(r) => r.groups[0].error_code,
            other => panic!("{other:?}"),
        }
    });
}

// ---- ShareFetch (KA L3274-3509: READ on the group, then READ on each topic it reads)

#[tokio::test]
async fn share_fetch_needs_read_on_the_group_and_on_each_topic() {
    five_ways!(
        &[
            need(GROUP, "grp", READ, GROUP_AUTH),
            need(TOPIC, "tpc", READ, TOPIC_AUTH)
        ],
        |rig, who| {
            let t = topic_id(rig, "tpc").await;
            let req = fetch_request("grp", &member_of(who), 0, &[(t, 0, vec![])]);
            match rig.ask_as(who, 1, Request::ShareFetch(req)).await {
                Response::ShareFetch(r) => first_code(
                    r.error_code,
                    r.responses
                        .iter()
                        .flat_map(|t| t.partitions.iter().map(|p| p.error_code)),
                ),
                other => panic!("{other:?}"),
            }
        }
    );
}

// ---- ShareAcknowledge (KA L3647-3731: READ on the group, then READ on each topic acknowledged)

#[tokio::test]
async fn share_acknowledge_needs_read_on_the_group_and_on_each_topic() {
    five_ways!(
        &[
            need(GROUP, "grp", READ, GROUP_AUTH),
            need(TOPIC, "tpc", READ, TOPIC_AUTH)
        ],
        |rig, who| {
            let t = topic_id(rig, "tpc").await;
            let member = member_of(who);
            let open = match rig
                .ask_as(
                    who,
                    1,
                    Request::ShareFetch(fetch_request("grp", &member, 0, &[])),
                )
                .await
            {
                Response::ShareFetch(r) => r.error_code,
                other => panic!("{other:?}"),
            };
            if open != 0 {
                open
            } else {
                let req = ack_request("grp", &member, 1, &[(t, 0, vec![(0, 0, vec![1])])]);
                match rig.ask_as(who, 1, Request::ShareAcknowledge(req)).await {
                    Response::ShareAcknowledge(r) => first_code(
                        r.error_code,
                        r.responses
                            .iter()
                            .flat_map(|t| t.partitions.iter().map(|p| p.error_code)),
                    ),
                    other => panic!("{other:?}"),
                }
            }
        }
    );
}

// ---- DescribeShareGroupOffsets (KA L3853-3970: DESCRIBE on the group, DESCRIBE on each topic named)

#[tokio::test]
async fn describe_share_group_offsets_needs_describe_on_the_group_and_on_each_topic() {
    five_ways!(
        &[
            need(GROUP, "grp", DESCRIBE, GROUP_AUTH),
            need(TOPIC, "tpc", DESCRIBE, TOPIC_AUTH)
        ],
        |rig, who| {
            make_topic(rig, "tpc", 1).await;
            let mut g = kafka_wire::messages::describe_share_group_offsets_request::DescribeShareGroupOffsetsRequestGroup::default();
            g.group_id = gx::group_id("grp");
            let mut t = kafka_wire::messages::describe_share_group_offsets_request::DescribeShareGroupOffsetsRequestTopic::default();
            t.topic_name = gx::topic_name("tpc");
            t.partitions = vec![0];
            g.topics = Some(vec![t]);
            let mut req = kafka_wire::messages::DescribeShareGroupOffsetsRequest::default();
            req.groups.push(g);
            match rig
                .ask_as(who, 1, Request::DescribeShareGroupOffsets(req))
                .await
            {
                Response::DescribeShareGroupOffsets(r) => first_code(
                    r.groups[0].error_code,
                    r.groups[0]
                        .topics
                        .iter()
                        .flat_map(|t| t.partitions.iter().map(|p| p.error_code)),
                ),
                other => panic!("{other:?}"),
            }
        }
    );
}

// ---- AlterShareGroupOffsets (KA L3972-4021: READ, not ALTER, on the group; READ on each topic)

#[tokio::test]
async fn alter_share_group_offsets_needs_read_on_the_group_and_on_each_topic() {
    five_ways!(
        &[
            need(GROUP, "grp", READ, GROUP_AUTH),
            need(TOPIC, "tpc", READ, TOPIC_AUTH)
        ],
        |rig, who| {
            make_topic(rig, "tpc", 1).await;
            let mut p = kafka_wire::messages::alter_share_group_offsets_request::AlterShareGroupOffsetsRequestPartition::default();
            p.partition_index = 0;
            let mut t = kafka_wire::messages::alter_share_group_offsets_request::AlterShareGroupOffsetsRequestTopic::default();
            t.topic_name = gx::topic_name("tpc");
            t.partitions.push(p);
            let mut req = kafka_wire::messages::AlterShareGroupOffsetsRequest::default();
            req.group_id = gx::group_id("grp");
            req.topics.push(t);
            match rig
                .ask_as(who, 0, Request::AlterShareGroupOffsets(req))
                .await
            {
                Response::AlterShareGroupOffsets(r) => first_code(
                    r.error_code,
                    r.responses
                        .iter()
                        .flat_map(|t| t.partitions.iter().map(|p| p.error_code)),
                ),
                other => panic!("{other:?}"),
            }
        }
    );
}

// ---- DeleteShareGroupOffsets (KA L4023-4077: DELETE on the group; READ on each topic)

#[tokio::test]
async fn delete_share_group_offsets_needs_delete_on_the_group_and_read_on_each_topic() {
    five_ways!(
        &[
            need(GROUP, "grp", DELETE, GROUP_AUTH),
            need(TOPIC, "tpc", READ, TOPIC_AUTH)
        ],
        |rig, who| {
            make_topic(rig, "tpc", 1).await;
            // The group is there (an empty share group the super user made), so the topics are looked at.
            let mut ap = kafka_wire::messages::alter_share_group_offsets_request::AlterShareGroupOffsetsRequestPartition::default();
            ap.partition_index = 0;
            let mut at = kafka_wire::messages::alter_share_group_offsets_request::AlterShareGroupOffsetsRequestTopic::default();
            at.topic_name = gx::topic_name("tpc");
            at.partitions.push(ap);
            let mut alter = kafka_wire::messages::AlterShareGroupOffsetsRequest::default();
            alter.group_id = gx::group_id("grp");
            alter.topics.push(at);
            rig.ask_as(&admin(), 0, Request::AlterShareGroupOffsets(alter))
                .await;
            let mut t = kafka_wire::messages::delete_share_group_offsets_request::DeleteShareGroupOffsetsRequestTopic::default();
            t.topic_name = gx::topic_name("tpc");
            let mut req = kafka_wire::messages::DeleteShareGroupOffsetsRequest::default();
            req.group_id = gx::group_id("grp");
            req.topics.push(t);
            match rig
                .ask_as(who, 0, Request::DeleteShareGroupOffsets(req))
                .await
            {
                Response::DeleteShareGroupOffsets(r) => {
                    first_code(r.error_code, r.responses.iter().map(|t| t.error_code))
                }
                other => panic!("{other:?}"),
            }
        }
    );
}

// ---- Group configs (ControllerApis L488-493: ALTER_CONFIGS on the group)

#[tokio::test]
async fn a_groups_configs_are_changed_with_alter_configs_on_the_group() {
    five_ways!(
        &[need(GROUP, "grp", ALTER_CONFIGS, GROUP_AUTH)],
        |rig, who| {
            let mut res = kafka_wire::messages::incremental_alter_configs_request::AlterConfigsResource::default();
            res.resource_type = 32;
            res.resource_name = gx::text("grp");
            let mut c =
                kafka_wire::messages::incremental_alter_configs_request::AlterableConfig::default();
            c.name = gx::text("share.auto.offset.reset");
            c.value = Some(gx::text("earliest"));
            res.configs.push(c);
            let mut req = kafka_wire::messages::IncrementalAlterConfigsRequest::default();
            req.resources.push(res);
            match rig
                .ask_as(who, 1, Request::IncrementalAlterConfigs(req))
                .await
            {
                Response::IncrementalAlterConfigs(r) => r.responses[0].error_code,
                other => panic!("{other:?}"),
            }
        }
    );
}
