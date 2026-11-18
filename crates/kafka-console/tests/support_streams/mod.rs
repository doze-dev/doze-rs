//! A Kafka Streams member for the live group tests (KIP-1071): raw StreamsGroupHeartbeat requests as a word count sends
//! them (subtopology 0 reads the input and re-keys into `<app>-words-repartition`; subtopology 1 reads that and counts
//! into a store with `<app>-counts-changelog`), the group configs that make the assignment come at once, and a commit at
//! the member's epoch.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::incremental_alter_configs_request::{
    AlterConfigsResource, AlterableConfig,
};
use kafka_wire::messages::streams_group_heartbeat_request::{
    KeyValue, Subtopology, TaskIds, TopicInfo, Topology,
};
use kafka_wire::messages::{
    GroupId, IncrementalAlterConfigsRequest, StreamsGroupHeartbeatRequest,
    StreamsGroupHeartbeatResponse, TopicName,
};

use crate::support_write::World;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

fn info(topic: &str, configs: &[(&str, &str)]) -> TopicInfo {
    let mut t = TopicInfo::default();
    t.name = TopicName(text(topic));
    t.replication_factor = -1;
    t.topic_configs = configs
        .iter()
        .map(|(k, v)| {
            let mut kv = KeyValue::default();
            kv.key = text(k);
            kv.value = text(v);
            kv
        })
        .collect();
    t
}

/// The word count Kafka Streams sends for application `app` reading `input`.
pub fn word_count(app: &str, input: &str) -> Topology {
    let rep = format!("{app}-words-repartition");
    let mut zero = Subtopology::default();
    zero.subtopology_id = text("0");
    zero.source_topics = vec![TopicName(text(input))];
    zero.repartition_sink_topics = vec![TopicName(text(&rep))];
    let mut one = Subtopology::default();
    one.subtopology_id = text("1");
    one.repartition_source_topics = vec![info(&rep, &[("cleanup.policy", "delete")])];
    one.state_changelog_topics = vec![info(
        &format!("{app}-counts-changelog"),
        &[("cleanup.policy", "compact")],
    )];
    let mut t = Topology::default();
    t.subtopologies = vec![zero, one];
    t
}

/// The group configs that make the first assignment come at once (no initial delay, no wait between assignments).
pub async fn quick(w: &World, group: &str) {
    let mut res = AlterConfigsResource::default();
    res.resource_type = 32;
    res.resource_name = text(group);
    res.configs = [
        ("streams.initial.rebalance.delay.ms", "0"),
        ("streams.assignment.interval.ms", "0"),
    ]
    .iter()
    .map(|(k, v)| {
        let mut c = AlterableConfig::default();
        c.name = text(k);
        c.value = Some(text(v));
        c
    })
    .collect();
    let mut req = IncrementalAlterConfigsRequest::default();
    req.resources.push(res);
    let Response::IncrementalAlterConfigs(r) =
        w.ask(1, Request::IncrementalAlterConfigs(req)).await
    else {
        panic!("not IncrementalAlterConfigs")
    };
    assert_eq!(r.responses[0].error_code, 0, "{r:?}");
}

fn heartbeat(group: &str, member: &str, epoch: i32) -> StreamsGroupHeartbeatRequest {
    let mut r = StreamsGroupHeartbeatRequest::default();
    r.group_id = GroupId(text(group));
    r.member_id = text(member);
    r.member_epoch = epoch;
    r.endpoint_information_epoch = -1;
    r
}

async fn beat(w: &World, req: StreamsGroupHeartbeatRequest) -> StreamsGroupHeartbeatResponse {
    let Response::StreamsGroupHeartbeat(r) =
        w.ask(1, Request::StreamsGroupHeartbeat(req.into())).await
    else {
        panic!("not StreamsGroupHeartbeat")
    };
    assert_eq!(r.error_code, 0, "{r:?}");
    *r
}

/// `member` joins `group` running the word count over `input`, as Kafka Streams does; the answer.
pub async fn join(
    w: &World,
    group: &str,
    member: &str,
    input: &str,
) -> StreamsGroupHeartbeatResponse {
    let mut r = heartbeat(group, member, 0);
    r.rebalance_timeout_ms = 300_000;
    r.topology = Some(word_count(group, input));
    r.active_tasks = Some(Vec::new());
    r.standby_tasks = Some(Vec::new());
    r.warmup_tasks = Some(Vec::new());
    r.process_id = Some(text(&format!("process-{member}")));
    let mut ep = kafka_wire::messages::streams_group_heartbeat_request::Endpoint::default();
    ep.host = text("localhost");
    ep.port = 7070;
    r.user_endpoint = Some(ep);
    r.client_tags = Some(Vec::new());
    r.task_offsets = Some(Vec::new());
    r.task_end_offsets = Some(Vec::new());
    beat(w, r).await
}

/// `member` joins and heartbeats, saying it runs what it was given, until it runs every task with no status; its epoch.
pub async fn settle(w: &World, group: &str, member: &str, input: &str) -> i32 {
    quick(w, group).await;
    let mut last = join(w, group, member, input).await;
    let mut owned: Vec<TaskIds> = Vec::new();
    for _ in 0..20 {
        if let Some(a) = &last.active_tasks {
            owned = a
                .iter()
                .map(|t| {
                    let mut x = TaskIds::default();
                    x.subtopology_id = t.subtopology_id.clone();
                    x.partitions.clone_from(&t.partitions);
                    x
                })
                .collect();
        }
        let mut hb = heartbeat(group, member, last.member_epoch);
        hb.active_tasks = Some(owned.clone());
        hb.standby_tasks = Some(Vec::new());
        hb.warmup_tasks = Some(Vec::new());
        last = beat(w, hb).await;
        let running = owned.iter().map(|t| t.partitions.len()).sum::<usize>() > 0;
        if running && last.status.as_ref().is_some_and(Vec::is_empty) {
            return last.member_epoch;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("{member} never ran its tasks: {last:?}")
}

/// A commit on `topic` by a streams member at its epoch (OffsetCommit v9, as Kafka Streams sends it).
pub async fn commit(
    w: &World,
    (group, member, epoch): (&str, &str, i32),
    topic: &str,
    offsets: &[(i32, i64)],
) {
    let Request::OffsetCommit(mut r) =
        kafka_console::backend::requests::admin_commit(group, topic, offsets)
    else {
        panic!()
    };
    r.generation_id_or_member_epoch = epoch;
    r.member_id = text(member);
    let Response::OffsetCommit(a) = w.ask(9, Request::OffsetCommit(r)).await else {
        panic!()
    };
    assert!(
        a.topics
            .iter()
            .flat_map(|t| &t.partitions)
            .all(|p| p.error_code == 0),
        "{a:?}"
    );
}
