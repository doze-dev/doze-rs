//! Streams groups (KIP-1071) through the broker: the heartbeat's checks in Kafka's order and words, the internal topics the
//! coordinator makes, the group among the other kinds (ListGroups, DescribeGroups, DeleteGroups), its offsets, its configs
//! and StreamsGroupTopologyDescriptionUpdate. The rules are docs/port/behaviours/streams-groups.md's.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::{
    DescribeGroupsRequest, GroupId, ListGroupsRequest, StreamsGroupTopologyDescriptionUpdateRequest,
};
use support::streams::{heartbeat, join, tasks};
use support::{rig, topic};

const INVALID_REQUEST: i16 = 42;
const UNKNOWN_MEMBER_ID: i16 = 25;
const GROUP_ID_NOT_FOUND: i16 = 69;
const NON_EMPTY_GROUP: i16 = 68;
const STREAMS_INVALID_TOPOLOGY: i16 = 130;
const UNSUPPORTED_VERSION: i16 = 35;
const INVALID_CONFIG: i16 = 40;

fn words(r: &kafka_wire::messages::StreamsGroupHeartbeatResponse) -> (i16, Option<String>) {
    (
        r.error_code,
        r.error_message.as_ref().map(ToString::to_string),
    )
}

#[tokio::test]
async fn the_heartbeats_own_checks_are_kafkas_in_its_order_and_words() {
    let rig = rig();
    let bad = |w: &str| (INVALID_REQUEST, Some(w.to_owned()));
    let mut r = join("g", "m", "in");
    r.rebalance_timeout_ms = -1;
    assert_eq!(
        words(&rig.streams_heartbeat(r).await),
        bad("RebalanceTimeoutMs must be provided in first request.")
    );
    let mut r = join("g", "m", "in");
    r.active_tasks = None;
    assert_eq!(
        words(&rig.streams_heartbeat(r).await),
        bad("ActiveTasks must be empty when (re-)joining.")
    );
    let mut r = join("g", "m", "in");
    r.topology = None;
    assert_eq!(
        words(&rig.streams_heartbeat(r).await),
        bad("Topology must be non-null when (re-)joining.")
    );
    let mut r = join("g", " ", "in");
    r.member_id = StrBytes::from_static_str(" ");
    assert_eq!(
        words(&rig.streams_heartbeat(r).await),
        bad("MemberId can't be empty.")
    );
    assert_eq!(
        words(&rig.streams_heartbeat(heartbeat("g", "m", -3)).await),
        bad("MemberEpoch is -3, but must be greater than or equal to -2.")
    );
    assert_eq!(
        words(&rig.streams_heartbeat(heartbeat("g", "m", -2)).await),
        bad("InstanceId can't be null.")
    );
    let mut r = heartbeat("g", "m", 3);
    r.active_tasks = Some(Vec::new());
    assert_eq!(
        words(&rig.streams_heartbeat(r).await),
        bad("If one task-type is non-null, all must be non-null.")
    );
    let mut r = join("g", "m", "in");
    r.member_epoch = 3;
    assert_eq!(
        words(&rig.streams_heartbeat(r).await),
        bad("Topology can only be provided when (re-)joining.")
    );
    let mut r = join("g", "m", "in");
    r.topology.as_mut().unwrap().subtopologies[0].source_topic_regex =
        vec![StrBytes::from_static_str("in.*")];
    assert_eq!(
        words(&rig.streams_heartbeat(r).await),
        bad("Regular expressions for source topics are not supported yet.")
    );
    let mut r = join("g", "m", "in");
    r.topology.as_mut().unwrap().subtopologies[1].state_changelog_topics[0].partitions = 3;
    assert_eq!(
        words(&rig.streams_heartbeat(r).await),
        (
            STREAMS_INVALID_TOPOLOGY,
            Some("Changelog topic g-counts-changelog must have an undefined partition count, but it is set to 3.".into())
        )
    );
}

#[tokio::test]
async fn kafkas_checks_of_the_topologys_names_and_of_the_group_come_with_their_words() {
    let rig = rig();
    // KafkaApis' own checks of the topology's names come first, with a code and its words.
    let r = join("g", "m", "__consumer_offsets");
    assert_eq!(
        words(&rig.streams_heartbeat(r).await),
        (
            STREAMS_INVALID_TOPOLOGY,
            Some("Use of Kafka internal topics __consumer_offsets in a Kafka Streams topology is prohibited.".into())
        )
    );
    let r = join("g", "m", "no spaces");
    assert_eq!(
        words(&rig.streams_heartbeat(r).await),
        (
            STREAMS_INVALID_TOPOLOGY,
            Some("Topic names no spaces are not valid topic names.".into())
        )
    );
    assert_eq!(
        words(&rig.streams_heartbeat(heartbeat("g", "m", 3)).await),
        (
            GROUP_ID_NOT_FOUND,
            Some("Streams group g not found.".into())
        )
    );
    assert_eq!(
        words(&rig.streams_heartbeat(heartbeat("g", "m", -1)).await),
        (GROUP_ID_NOT_FOUND, Some("Group g not found.".into()))
    );
}

#[tokio::test]
async fn the_coordinator_makes_the_internal_topics_with_the_partitions_and_configs_the_topology_says()
 {
    let rig = rig();
    rig.create(vec![topic("in", 3)], false).await;
    let quick = [
        ("streams.initial.rebalance.delay.ms", "0"),
        ("streams.assignment.interval.ms", "0"),
    ];
    assert_eq!(rig.group_config("app", &quick).await, 0);
    let first = rig.streams_heartbeat(join("app", "m", "in")).await;
    assert_eq!(
        first.member_epoch, 2,
        "no delay: the targets are computed at once"
    );
    let status: Vec<(i8, String)> = first
        .status
        .unwrap()
        .into_iter()
        .map(|s| (s.status_code, s.status_detail.to_string()))
        .collect();
    assert_eq!(
        status[0],
        (
            3,
            "Internal topics are missing: app-counts-changelog, app-words-repartition".into()
        )
    );
    let meta = rig
        .metadata(
            12,
            Some(&["app-words-repartition", "app-counts-changelog"]),
            false,
        )
        .await;
    for t in &meta.topics {
        assert_eq!((t.error_code, t.partitions.len()), (0, 3), "{t:?}");
    }
    let changelog = rig.topic_configs("app-counts-changelog").await;
    assert_eq!(
        changelog.get("cleanup.policy").map(String::as_str),
        Some("compact")
    );
    let settled = rig.streams_settle("app", "m", "in").await;
    let actives: usize = settled
        .active_tasks
        .iter()
        .flatten()
        .map(|t| t.partitions.len())
        .sum();
    assert!(
        actives == 6 || settled.active_tasks.is_none(),
        "{settled:?}"
    );
    let d = rig.streams_describe(&["app"]).await.groups.remove(0);
    assert_eq!((d.error_code, d.group_state.as_str()), (0, "Stable"));
    assert_eq!(
        d.topology
            .as_ref()
            .unwrap()
            .subtopologies
            .as_ref()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn a_streams_group_is_listed_as_one_and_not_described_by_the_classic_api() {
    let rig = rig();
    rig.create(vec![topic("in", 1)], false).await;
    rig.streams_settle("app", "m", "in").await;
    let mut req = ListGroupsRequest::default();
    req.types_filter = vec![StrBytes::from_static_str("streams")];
    let Response::ListGroups(listed) = rig.ask(5, Request::ListGroups(req)).await else {
        panic!()
    };
    let g = &listed.groups[0];
    assert_eq!(
        (
            g.group_id.0.as_str(),
            g.protocol_type.as_str(),
            g.group_type.as_str()
        ),
        ("app", "streams", "streams")
    );
    let mut req = ListGroupsRequest::default();
    req.states_filter = vec![StrBytes::from_static_str("NotReady")];
    let Response::ListGroups(none) = rig.ask(5, Request::ListGroups(req)).await else {
        panic!()
    };
    assert!(none.groups.is_empty());
    let mut req = DescribeGroupsRequest::default();
    req.groups = vec![GroupId(StrBytes::from_static_str("app"))];
    let Response::DescribeGroups(d) = rig.ask(5, Request::DescribeGroups(req)).await else {
        panic!()
    };
    assert_eq!(d.groups[0].group_state.as_str(), "Dead");
    // A share heartbeat to it is refused by kind.
    let share = rig.share_join("app", "s", &["in"]).await;
    assert_eq!(
        share.error_message.map(|m| m.to_string()),
        Some("Group app is not a share group.".into())
    );
}

#[tokio::test]
async fn a_streams_group_with_members_is_not_deleted_and_an_empty_one_is() {
    let rig = rig();
    rig.create(vec![topic("in", 1)], false).await;
    let last = rig.streams_settle("app", "m", "in").await;
    assert_eq!(
        rig.delete_groups(&["app"]).await,
        [("app".to_owned(), NON_EMPTY_GROUP)]
    );
    let left = rig.streams_heartbeat(heartbeat("app", "m", -1)).await;
    assert_eq!((left.error_code, left.member_epoch), (0, -1));
    assert_eq!(rig.delete_groups(&["app"]).await, [("app".to_owned(), 0)]);
    let d = rig.streams_describe(&["app"]).await.groups.remove(0);
    assert_eq!(d.error_code, GROUP_ID_NOT_FOUND);
    // Its internal topics stay: only the tool deletes them.
    assert!(rig.has_topic("app-counts-changelog").await);
    let _ = last;
}

#[tokio::test]
async fn an_unknown_member_and_a_group_of_another_kind_are_refused_in_kafkas_words() {
    let rig = rig();
    rig.create(vec![topic("in", 1)], false).await;
    rig.streams_settle("app", "m", "in").await;
    let r = rig.streams_heartbeat(heartbeat("app", "x", 4)).await;
    assert_eq!(
        words(&r),
        (
            UNKNOWN_MEMBER_ID,
            Some("Member x is not a member of group app.".into())
        )
    );
    assert_eq!(rig.share_join("sg", "s", &["in"]).await.error_code, 0);
    let r = rig.streams_heartbeat(join("sg", "m", "in")).await;
    assert_eq!(
        words(&r),
        (
            GROUP_ID_NOT_FOUND,
            Some("Group sg is not a streams group.".into())
        )
    );
    let mut owned = heartbeat("app", "m", 99);
    owned.active_tasks = Some(tasks(&[("0", &[0])]));
    owned.standby_tasks = Some(Vec::new());
    owned.warmup_tasks = Some(Vec::new());
    assert_eq!(
        rig.streams_heartbeat(owned).await.error_code,
        110,
        "a greater epoch is fenced"
    );
}

#[tokio::test]
async fn the_streams_group_configs_are_kept_checked_and_described() {
    let rig = rig();
    assert_eq!(
        rig.group_config("app", &[("streams.num.standby.replicas", "2")])
            .await,
        0
    );
    assert_eq!(
        rig.group_config("app", &[("streams.num.standby.replicas", "3")])
            .await,
        INVALID_CONFIG
    );
    assert_eq!(
        rig.group_config("app", &[("streams.session.timeout.ms", "30000")])
            .await,
        INVALID_CONFIG
    );
    assert_eq!(
        rig.group_config("app", &[("streams.assignor.name", "range")])
            .await,
        INVALID_CONFIG
    );
    assert_eq!(
        rig.group_config(
            "app",
            &[("streams.rack.aware.assignment.tags", "zone,zone")]
        )
        .await,
        INVALID_CONFIG
    );
    assert_eq!(
        rig.group_config("app", &[("streams.task.offset.interval.ms", "1000")])
            .await,
        INVALID_CONFIG
    );
    assert_eq!(
        rig.group_config("app", &[("streams.acceptable.recovery.lag", "20000")])
            .await,
        0
    );
    let described = rig.describe_group_configs("app").await;
    assert_eq!(
        described
            .get("streams.num.standby.replicas")
            .map(|c| (c.0.as_str(), c.1)),
        Some(("2", 8))
    );
    assert_eq!(
        described
            .get("streams.heartbeat.interval.ms")
            .map(|c| (c.0.as_str(), c.1)),
        Some(("5000", 5))
    );
    assert_eq!(
        described.get("streams.assignor.name").map(|c| c.0.as_str()),
        Some("sticky")
    );
    // Internal configs are listed only where the group sets them.
    assert!(described.contains_key("streams.acceptable.recovery.lag"));
    assert!(!described.contains_key("streams.num.warmup.replicas"));
}

#[tokio::test]
async fn the_topology_description_update_is_refused_as_by_a_broker_with_no_plugin() {
    let rig = rig();
    let mut req = StreamsGroupTopologyDescriptionUpdateRequest::default();
    req.group_id = GroupId(StrBytes::from_static_str("app"));
    req.member_id = StrBytes::from_static_str("m");
    let Response::StreamsGroupTopologyDescriptionUpdate(r) = rig
        .ask(0, Request::StreamsGroupTopologyDescriptionUpdate(req))
        .await
    else {
        panic!()
    };
    assert_eq!(r.error_code, UNSUPPORTED_VERSION);
    assert_eq!(
        r.error_message.map(|m| m.to_string()),
        Some("The broker has no streams group topology description plugin configured.".into())
    );
}

#[tokio::test]
async fn version_zero_answers_carry_no_long_recovery_lag_and_version_one_answers_do() {
    let rig = rig();
    rig.create(vec![topic("in", 1)], false).await;
    let v0 = rig.streams_heartbeat_at(0, join("app", "m0", "in")).await;
    assert_eq!((v0.error_code, v0.acceptable_recovery_lag_legacy), (0, 0));
    let v1 = rig.streams_heartbeat_at(1, join("app", "m1", "in")).await;
    assert_eq!(
        (
            v1.acceptable_recovery_lag,
            v1.task_offset_interval_ms,
            v1.heartbeat_interval_ms
        ),
        (10_000, 60_000, 5_000)
    );
}

/// STR-73: the offsets of a topic the topology reads cannot be deleted while the group has members; a changelog's can.
#[tokio::test]
async fn offsets_of_a_topic_the_topology_reads_are_not_deleted_while_it_has_members() {
    const GROUP_SUBSCRIBED_TO_TOPIC: i16 = 86;
    let rig = rig();
    rig.create(vec![topic("in", 1)], false).await;
    rig.streams_settle("app", "m", "in").await;
    assert_eq!(
        rig.offset_delete("app", &[("in", 0)]).await,
        (0, vec![GROUP_SUBSCRIBED_TO_TOPIC])
    );
    assert_eq!(
        rig.offset_delete("app", &[("app-words-repartition", 0)])
            .await,
        (0, vec![GROUP_SUBSCRIBED_TO_TOPIC])
    );
    assert_eq!(
        rig.offset_delete("app", &[("app-counts-changelog", 0)])
            .await,
        (0, vec![0])
    );
}
