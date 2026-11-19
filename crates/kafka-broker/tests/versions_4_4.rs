//! The versions Kafka 4.4.0 added to APIs the broker already served (debt K1, 2026-11-19), each held to what a real 4.4.0
//! answered (`tools/wire-tail-oracle`, docs/port/behaviours/wire.md W37): ListOffsets 8 to 11, OffsetCommit and OffsetFetch
//! 10 (topics by id), DescribeGroups 6, ListTransactions 2, FindCoordinator 6.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_wire::StrBytes;
use kafka_wire::Uuid;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_wire::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_wire::messages::offset_fetch_request::{
    OffsetFetchRequestGroup, OffsetFetchRequestTopics,
};
use kafka_wire::messages::{
    BrokerId, DescribeGroupsRequest, DescribeGroupsResponse, FindCoordinatorRequest,
    FindCoordinatorResponse, GroupId, ListOffsetsRequest, ListOffsetsResponse,
    ListTransactionsRequest, ListTransactionsResponse, OffsetCommitRequest, OffsetCommitResponse,
    OffsetFetchRequest, OffsetFetchResponse,
};
use support::acls::{closed_world, user};
use support::{Rig, records, rig, rig_with, topic, wire_name};

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const CLUSTER_AUTHORIZATION_FAILED: i16 = 31;
const UNSUPPORTED_VERSION: i16 = 35;
const INVALID_REQUEST: i16 = 42;
const GROUP_ID_NOT_FOUND: i16 = 69;
const UNKNOWN_TOPIC_ID: i16 = 100;
const INVALID_REGULAR_EXPRESSION: i16 = 128;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// `(error, offset, leader epoch)` of a ListOffsets of one partition at `version`.
async fn list(
    rig: &Rig,
    version: i16,
    name: &str,
    timestamp: i64,
    isolation: i8,
) -> (i16, i64, i32) {
    let mut p = ListOffsetsPartition::default();
    p.partition_index = 0;
    p.timestamp = timestamp;
    p.current_leader_epoch = -1;
    let mut t = ListOffsetsTopic::default();
    t.name = wire_name(name);
    t.partitions.push(p);
    let mut req = ListOffsetsRequest::default();
    req.replica_id = BrokerId(-1);
    req.isolation_level = isolation;
    req.topics.push(t);
    let Response::ListOffsets(r) = rig.ask(version, Request::ListOffsets(req)).await else {
        panic!("not ListOffsets")
    };
    let ListOffsetsResponse { topics, .. } = r;
    let a = &topics[0].partitions[0];
    (a.error_code, a.offset, a.leader_epoch)
}

/// The oracle's `list-offsets` lines: the local start (-4, v8) is the start; the tiered times (-5 from v9, -6 from v11) are
/// offset -1 at leader epoch -1 with no tiered storage; a time below the version that brought it, or one no version has, is
/// UNSUPPORTED_VERSION, and that before the topic is looked for.
#[tokio::test]
async fn the_tiered_storage_times_are_answered_as_kafka_4_4_answers_them_with_none() {
    let rig = rig();
    rig.create(vec![topic("t", 1), topic("empty", 1)], false)
        .await;
    rig.produce(-1, "t", 0, records(1)).await;
    rig.produce(-1, "t", 0, records(1)).await;
    let ok = |offset, epoch| (0, offset, epoch);
    assert_eq!(list(&rig, 11, "t", -1, 0).await, ok(2, 0));
    assert_eq!(list(&rig, 11, "t", -4, 0).await, ok(0, 0));
    assert_eq!(list(&rig, 11, "t", -5, 0).await, ok(-1, -1));
    assert_eq!(list(&rig, 11, "t", -6, 0).await, ok(-1, -1));
    assert_eq!(
        list(&rig, 11, "t", -6, 1).await,
        ok(-1, -1),
        "read_committed alike"
    );
    assert_eq!(list(&rig, 11, "empty", -4, 0).await, ok(0, 0));
    assert_eq!(list(&rig, 11, "empty", -5, 0).await, ok(-1, -1));
    assert_eq!(list(&rig, 9, "t", -5, 0).await, ok(-1, -1));
    let unsupported = (UNSUPPORTED_VERSION, -1, -1);
    assert_eq!(list(&rig, 7, "t", -4, 0).await, unsupported);
    assert_eq!(list(&rig, 8, "t", -5, 0).await, unsupported);
    assert_eq!(list(&rig, 10, "t", -6, 0).await, unsupported);
    assert_eq!(list(&rig, 11, "t", -7, 0).await, unsupported);
    assert_eq!(
        list(&rig, 10, "nothing", -6, 0).await,
        unsupported,
        "before the topic"
    );
    assert_eq!(
        list(&rig, 11, "nothing", -6, 0).await,
        (UNKNOWN_TOPIC_OR_PARTITION, -1, -1)
    );
}

fn commit_request(group: &str, id: Uuid, name: &str, partitions: &[i32]) -> OffsetCommitRequest {
    let mut t = OffsetCommitRequestTopic::default();
    t.name = wire_name(name);
    t.topic_id = id;
    for &p in partitions {
        let mut part = OffsetCommitRequestPartition::default();
        part.partition_index = p;
        part.committed_offset = 5 + i64::from(p);
        part.committed_leader_epoch = -1;
        part.committed_metadata = Some(text(&format!("m{p}")));
        t.partitions.push(part);
    }
    let mut req = OffsetCommitRequest::default();
    req.group_id = GroupId(text(group));
    req.member_id = text("");
    req.generation_id_or_member_epoch = -1;
    req.topics.push(t);
    req
}

/// `(topic id, name, partition, code)` of each partition of an OffsetCommit answer.
async fn commit(
    rig: &Rig,
    version: i16,
    req: OffsetCommitRequest,
) -> Vec<(Uuid, String, i32, i16)> {
    let Response::OffsetCommit(r) = rig.ask(version, Request::OffsetCommit(req)).await else {
        panic!("not OffsetCommit")
    };
    let OffsetCommitResponse { topics, .. } = r;
    topics
        .iter()
        .flat_map(|t| {
            t.partitions.iter().map(|p| {
                (
                    t.topic_id,
                    t.name.0.to_string(),
                    p.partition_index,
                    p.error_code,
                )
            })
        })
        .collect()
}

/// `(topic id, partition, code, offset)` of each partition of an OffsetFetch v8+ answer of one group.
async fn fetch(
    rig: &Rig,
    version: i16,
    group: &str,
    ids: Option<&[Uuid]>,
) -> Vec<(Uuid, i32, i16, i64)> {
    let mut g = OffsetFetchRequestGroup::default();
    g.group_id = GroupId(text(group));
    g.topics = ids.map(|ids| {
        ids.iter()
            .map(|id| {
                let mut t = OffsetFetchRequestTopics::default();
                t.topic_id = *id;
                t.partition_indexes = vec![0, 1];
                t
            })
            .collect()
    });
    let mut req = OffsetFetchRequest::default();
    req.require_stable = true;
    req.groups.push(g);
    let Response::OffsetFetch(r) = rig.ask(version, Request::OffsetFetch(req)).await else {
        panic!("not OffsetFetch")
    };
    let OffsetFetchResponse { groups, .. } = r;
    assert_eq!(groups[0].error_code, 0);
    groups[0]
        .topics
        .iter()
        .flat_map(|t| {
            t.partitions.iter().map(|p| {
                (
                    t.topic_id,
                    p.partition_index,
                    p.error_code,
                    p.committed_offset,
                )
            })
        })
        .collect()
}

/// The oracle's `offsets-10` lines: v10 names topics by id; a known id is committed and fetched under its name, its
/// partition out of range is 3, an id no topic has (the nil id included) is 100 and is never put to the group; fetching
/// every offset at v10 answers each topic by its id.
#[tokio::test]
async fn offset_commit_and_fetch_v10_name_topics_by_id() {
    let rig = rig();
    let made = rig.create(vec![topic("t", 2)], false).await;
    let id = made[0].topic_id;
    let nobody = Uuid::from_u128(0xfeed);
    let committed = commit(&rig, 10, commit_request("g", id, "", &[0, 1, 2])).await;
    let codes: Vec<_> = committed.iter().map(|c| (c.0, c.2, c.3)).collect();
    // In Kafka's order: what KafkaApis answers itself first, then what the group answered.
    assert_eq!(
        codes,
        [(id, 2, UNKNOWN_TOPIC_OR_PARTITION), (id, 0, 0), (id, 1, 0)]
    );
    for unknown in [nobody, Uuid::nil()] {
        let refused = commit(&rig, 10, commit_request("g", unknown, "t", &[0])).await;
        assert_eq!(
            refused.iter().map(|c| (c.0, c.3)).collect::<Vec<_>>(),
            [(unknown, UNKNOWN_TOPIC_ID)],
            "an unknown id is unknown whatever name the entry also carries"
        );
    }
    // v9 names by name, as before: the id an entry also carries is not read.
    let by_name = commit(&rig, 9, commit_request("g", nobody, "t", &[1])).await;
    assert_eq!(by_name[0].3, 0);
    assert_eq!(
        fetch(&rig, 10, "g", Some(&[id, nobody])).await,
        [
            (id, 0, 0, 5),
            (id, 1, 0, 6),
            (nobody, 0, UNKNOWN_TOPIC_ID, -1),
            (nobody, 1, UNKNOWN_TOPIC_ID, -1)
        ]
    );
    assert_eq!(
        fetch(&rig, 10, "g", None).await,
        [(id, 0, 0, 5), (id, 1, 0, 6)]
    );
    assert_eq!(
        fetch(&rig, 10, "g-none", Some(&[id])).await,
        [(id, 0, 0, -1), (id, 1, 0, -1)]
    );
}

async fn describe(
    rig: &Rig,
    version: i16,
    groups: &[&str],
) -> Vec<(String, i16, String, Option<String>)> {
    let mut req = DescribeGroupsRequest::default();
    req.groups = groups.iter().map(|g| GroupId(text(g))).collect();
    let Response::DescribeGroups(r) = rig.ask(version, Request::DescribeGroups(req)).await else {
        panic!("not DescribeGroups")
    };
    let DescribeGroupsResponse { groups, .. } = r;
    let mut out: Vec<_> = groups
        .iter()
        .map(|g| {
            (
                g.group_id.0.to_string(),
                g.error_code,
                g.group_state.to_string(),
                g.error_message.as_ref().map(ToString::to_string),
            )
        })
        .collect();
    out.sort();
    out
}

/// The oracle's `describe-groups` lines: from v6 a group not found (the empty id included) is Dead with
/// GROUP_ID_NOT_FOUND and Kafka's words; up to v5 it is Dead with no error. A group known by its offsets is Empty.
#[tokio::test]
async fn describe_groups_v6_says_a_group_is_not_found() {
    let rig = rig();
    let made = rig.create(vec![topic("t", 1)], false).await;
    commit(
        &rig,
        10,
        commit_request("empty", made[0].topic_id, "", &[0]),
    )
    .await;
    let found = |g: &str| (g.to_owned(), 0, "Empty".to_owned(), None);
    let dead = |g: &str| (g.to_owned(), 0, "Dead".to_owned(), None);
    let missing = |g: &str| {
        (
            g.to_owned(),
            GROUP_ID_NOT_FOUND,
            "Dead".to_owned(),
            Some(format!("Group {g} not found.")),
        )
    };
    assert_eq!(
        describe(&rig, 6, &["none", "empty", ""]).await,
        [missing(""), found("empty"), missing("none")]
    );
    assert_eq!(
        describe(&rig, 5, &["none", "empty", ""]).await,
        [dead(""), found("empty"), dead("none")]
    );
}

async fn list_txns(rig: &Rig, version: i16, pattern: Option<&str>) -> (i16, Vec<String>) {
    let mut req = ListTransactionsRequest::default();
    req.transactional_id_pattern = pattern.map(text);
    let Response::ListTransactions(r) = rig.ask(version, Request::ListTransactions(req)).await
    else {
        panic!("not ListTransactions")
    };
    let ListTransactionsResponse {
        error_code,
        transaction_states,
        ..
    } = r;
    let mut ids: Vec<_> = transaction_states
        .iter()
        .map(|s| s.transactional_id.0.to_string())
        .collect();
    ids.sort();
    (error_code, ids)
}

/// The oracle's `list-transactions` lines: a v2 pattern keeps the ids it matches whole; null or empty keeps all; one that
/// does not compile is 128 with no rows.
#[tokio::test]
async fn list_transactions_v2_keeps_the_ids_its_pattern_matches_whole() {
    let rig = rig();
    rig.init_txn("lt-alpha", 60_000).await.unwrap();
    rig.init_txn("lt-beta", 60_000).await.unwrap();
    let both = (0, vec!["lt-alpha".to_owned(), "lt-beta".to_owned()]);
    assert_eq!(list_txns(&rig, 1, None).await, both);
    assert_eq!(list_txns(&rig, 2, None).await, both);
    assert_eq!(list_txns(&rig, 2, Some("")).await, both);
    assert_eq!(
        list_txns(&rig, 2, Some(".*alph.")).await,
        (0, vec!["lt-alpha".to_owned()])
    );
    assert_eq!(
        list_txns(&rig, 2, Some("alpha")).await,
        (0, vec![]),
        "a part is not a match"
    );
    assert_eq!(list_txns(&rig, 2, Some("lt-(alpha|beta)")).await, both);
    assert_eq!(
        list_txns(&rig, 2, Some("(unclosed")).await,
        (INVALID_REGULAR_EXPRESSION, vec![])
    );
}

async fn coordinators(
    rig: &Rig,
    who: &kafka_broker::Caller,
    version: i16,
    key_type: i8,
    keys: &[&str],
) -> Vec<(String, i16, i32, Option<String>)> {
    let mut req = FindCoordinatorRequest::default();
    req.key_type = key_type;
    req.coordinator_keys = keys.iter().map(|k| text(k)).collect();
    let Response::FindCoordinator(r) = rig
        .ask_as(who, version, Request::FindCoordinator(req))
        .await
    else {
        panic!("not FindCoordinator")
    };
    let FindCoordinatorResponse { coordinators, .. } = r;
    coordinators
        .iter()
        .map(|c| {
            (
                c.key.to_string(),
                c.error_code,
                c.node_id.0,
                c.error_message.as_ref().map(ToString::to_string),
            )
        })
        .collect()
}

/// The oracle's `find-coordinator` lines: a share partition's key (type 2, `group:topicId:partition`, read from the right)
/// is this broker from v6, INVALID_REQUEST before v6 and for a key not of that form; a caller without CLUSTER_ACTION is
/// refused 31 on every key, with Kafka's words.
#[tokio::test]
async fn find_coordinator_v6_answers_a_share_partition_key() {
    let rig = rig();
    let me = support::caller();
    let id = "0bFg2GHCR_67Zo0_m2MTGQ";
    let good = format!("g:{id}:0");
    let colons = format!("a:b:{id}:3");
    let negative = format!("g:{id}:-1");
    let node = rig.broker.identity().1;
    let ok = |k: &str| (k.to_owned(), 0, node, Some(String::new()));
    let bad = |k: &str| (k.to_owned(), INVALID_REQUEST, -1, Some(String::new()));
    assert_eq!(
        coordinators(&rig, &me, 6, 2, &[&good, &colons, &negative]).await,
        [ok(&good), ok(&colons), ok(&negative)]
    );
    assert_eq!(coordinators(&rig, &me, 5, 2, &[&good]).await, [bad(&good)]);
    let not_a_number = format!("g:{id}:x");
    let blank_group = format!(" :{id}:0");
    let bad_keys = ["x", "g:notanid:0", &not_a_number, &blank_group, ""];
    assert_eq!(
        coordinators(&rig, &me, 6, 2, &bad_keys).await,
        bad_keys.iter().map(|k| bad(k)).collect::<Vec<_>>()
    );
    let closed = rig_with(closed_world);
    assert_eq!(
        coordinators(&closed, &user("bob"), 6, 2, &[&good, "x"]).await,
        [&good, "x"]
            .iter()
            .map(|k| (
                (*k).to_owned(),
                CLUSTER_AUTHORIZATION_FAILED,
                -1,
                Some("Cluster authorization failed.".to_owned())
            ))
            .collect::<Vec<_>>()
    );
}
