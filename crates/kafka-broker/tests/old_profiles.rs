//! The older profiles (debt K2, 2026-11-19): `--kafka-version` 1, 2 and 3 are Kafka 1.1, 2.8 and 3.9, and serve every version
//! their recorded broker lists, as that release behaves (docs/port/behaviours/wire.md W38, fetch.md F12 and L3, produce.md
//! P16, groups.md G11). What a real broker answered is held in `tools/old-profiles-oracle` and the recorded `old-*` replays;
//! these pin the broker's side of it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use bytes::Bytes;
use kafka_broker::Outcome;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::legacy::{self, Magic};
use kafka_wire::messages::fetch_response::PartitionData;
use kafka_wire::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_wire::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_wire::messages::offset_fetch_request::OffsetFetchRequestTopic;
use kafka_wire::messages::{
    BrokerId, ConsumerGroupHeartbeatRequest, FetchRequest, FetchResponse, GroupId,
    ListOffsetsRequest, OffsetCommitRequest, OffsetFetchRequest,
};
use kafka_wire::profile::KafkaMajor;
use support::fetch_request;
use support::{Rig, rig_with, topic, wire_name};
use support::{produce_request, records};

const CORRUPT_MESSAGE: i16 = 2;
const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const UNSUPPORTED_VERSION: i16 = 35;
const UNSUPPORTED_COMPRESSION_TYPE: i16 = 76;
const INVALID_RECORD: i16 = 87;

async fn kafka(major: u8) -> Rig {
    let rig = rig_with(|c| c.kafka_major = KafkaMajor::new(major).unwrap());
    rig.create(vec![topic("t", 2)], false).await;
    rig
}

fn set(magic: Magic, n: usize) -> Bytes {
    let ts = 1_795_000_000_000_i64;
    let keys: Vec<String> = (0..n).map(|i| format!("k{i}")).collect();
    let msgs: Vec<legacy::TestMessage<'_>> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| (ts + i as i64, Some(k.as_bytes()), Some(&b"v"[..])))
        .collect();
    legacy::message_set(magic, &msgs)
}

/// (error, base offset) of a Produce of `recs` to t-0 at `version`.
async fn produce(rig: &Rig, version: i16, recs: Bytes) -> (i16, i64) {
    let Response::Produce(r) = rig
        .ask(
            version,
            Request::Produce(produce_request(-1, "t", &[(0, recs)])),
        )
        .await
    else {
        panic!("not Produce")
    };
    let p = &r.responses[0].partition_responses[0];
    (p.error_code, p.base_offset)
}

async fn fetch(rig: &Rig, version: i16, offset: i64, limit: i32) -> PartitionData {
    let mut req: FetchRequest = fetch_request(&[("t", 0, offset, limit)], 0, 0, 1 << 20);
    req.replica_id = BrokerId(-1);
    let Response::Fetch(FetchResponse { responses, .. }) =
        rig.ask(version, Request::Fetch(req)).await
    else {
        panic!("not Fetch")
    };
    responses.into_iter().next().unwrap().partitions.remove(0)
}

#[tokio::test]
async fn produce_v0_to_v2_message_sets_are_stored_as_v2_batches_with_their_times() {
    let rig = kafka(2).await;
    assert_eq!(produce(&rig, 0, set(Magic::V0, 2)).await, (0, 0));
    assert_eq!(produce(&rig, 1, set(Magic::V1, 2)).await, (0, 2));
    assert_eq!(
        produce(&rig, 2, records(1)).await,
        (0, 4),
        "a v2 batch at v2"
    );
    assert_eq!(
        produce(&rig, 2, Bytes::new()).await,
        (0, -1),
        "nothing appends nothing"
    );
    // Two formats in one set, from Kafka 2.0.
    let mixed = Bytes::from([set(Magic::V0, 1), set(Magic::V1, 1)].concat());
    assert_eq!(produce(&rig, 0, mixed.clone()).await.0, INVALID_RECORD);
    let p = fetch(&rig, 4, 0, 1 << 20).await;
    let records = p.records.unwrap();
    let batches: Vec<_> = kafka_wire::batch::Batches::new(records)
        .map(Result::unwrap)
        .collect();
    assert_eq!(batches.len(), 3);
    assert_eq!(
        batches[0].max_timestamp().get(),
        -1,
        "format v0 has no time"
    );
    assert_eq!(batches[1].max_timestamp().get(), 1_795_000_000_001);
    // Kafka 1.1 converts the mix, and says CORRUPT_MESSAGE where later releases say INVALID_RECORD.
    let one = kafka(1).await;
    assert_eq!(produce(&one, 0, mixed).await, (0, 0));
    let mut bad = set(Magic::V1, 1).to_vec();
    let last = bad.len() - 1;
    bad[last] ^= 0x5a;
    assert_eq!(produce(&one, 1, Bytes::from(bad)).await.0, CORRUPT_MESSAGE);
}

#[tokio::test]
async fn kafka_1_closes_the_connection_on_a_v3_request_that_is_not_one_v2_batch() {
    let one = kafka(1).await;
    let req = produce_request(-1, "t", &[(0, set(Magic::V1, 1))]);
    let head = support::head(kafka_wire::messages::ApiKey::Produce, 3);
    let outcome = one
        .broker
        .handle(
            &support::at(),
            &support::caller(),
            &head,
            Request::Produce(req),
        )
        .await;
    assert!(matches!(outcome, Outcome::Close), "{outcome:?}");
    let two = kafka(2).await;
    assert_eq!(produce(&two, 3, set(Magic::V1, 1)).await.0, INVALID_RECORD);
}

#[tokio::test]
async fn fetch_v0_to_v3_read_down_converted_messages_sized_as_the_release_sizes_them() {
    for major in [1, 2, 3] {
        let rig = kafka(major).await;
        produce(&rig, 3, records(3)).await;
        let stored = fetch(&rig, 4, 1, 1 << 20).await.records.unwrap().len();
        for (version, magic) in [
            (0, Magic::V0),
            (1, Magic::V0),
            (2, Magic::V1),
            (3, Magic::V1),
        ] {
            let p = fetch(&rig, version, 1, 1 << 20).await;
            assert_eq!(p.error_code, 0);
            let read = p.records.unwrap();
            let msgs = legacy::read_set(&read).unwrap_or_else(|| panic!("v{version}: {read:?}"));
            let offsets: Vec<i64> = msgs.iter().map(|m| m.1).collect();
            assert_eq!(
                offsets,
                [1, 2],
                "Kafka {major} v{version}: from the fetch offset"
            );
            assert!(msgs.iter().all(|m| m.0 == magic));
            // From Kafka 2.0 the answer is at least as long as the read, filled with overflow.
            if major >= 2 {
                assert!(
                    read.len() >= stored,
                    "Kafka {major} v{version}: {} < {stored}",
                    read.len()
                );
            }
        }
    }
}

#[tokio::test]
async fn a_zstd_topic_is_not_read_below_fetch_v10_from_kafka_2() {
    let rig = rig_with(|c| c.kafka_major = KafkaMajor::new(2).unwrap());
    let mut t = topic("t", 1);
    let mut c = kafka_wire::messages::create_topics_request::CreatableTopicConfig::default();
    c.name = StrBytes::from_static_str("compression.type");
    c.value = Some(StrBytes::from_static_str("zstd"));
    t.configs.push(c);
    rig.create(vec![t], false).await;
    for version in [0, 3, 4, 9] {
        assert_eq!(
            fetch(&rig, version, 0, 1 << 20).await.error_code,
            UNSUPPORTED_COMPRESSION_TYPE,
            "v{version}"
        );
    }
    assert_eq!(fetch(&rig, 10, 0, 1 << 20).await.error_code, 0);
}

#[tokio::test]
async fn list_offsets_v0_answers_offsets_newest_first() {
    let rig = kafka(2).await;
    produce(&rig, 3, records(3)).await;
    let ask = |ts: i64, n: i32| {
        let mut p = ListOffsetsPartition::default();
        p.timestamp = ts;
        p.max_num_offsets = n;
        let mut t = ListOffsetsTopic::default();
        t.name = wire_name("t");
        t.partitions.push(p);
        let mut req = ListOffsetsRequest::default();
        req.replica_id = BrokerId(-1);
        req.topics.push(t);
        req
    };
    for (ts, n, want) in [
        (-1, 1, vec![3]),
        (-1, 5, vec![3, 0]),
        (-2, 5, vec![0]),
        (-1, 0, vec![]),
    ] {
        let Response::ListOffsets(r) = rig.ask(0, Request::ListOffsets(ask(ts, n))).await else {
            panic!()
        };
        assert_eq!(
            r.topics[0].partitions[0].old_style_offsets, want,
            "{ts} {n}"
        );
    }
}

fn commit(partition: i32, offset: i64) -> OffsetCommitRequest {
    let mut p = OffsetCommitRequestPartition::default();
    p.partition_index = partition;
    p.committed_offset = offset;
    let mut t = OffsetCommitRequestTopic::default();
    t.name = wire_name("t");
    t.partitions.push(p);
    let mut req = OffsetCommitRequest::default();
    req.group_id = GroupId(StrBytes::from_static_str("g"));
    req.generation_id_or_member_epoch = -1;
    req.topics.push(t);
    req
}

async fn committed(rig: &Rig, version: i16) -> Vec<(i32, i16, i64)> {
    let mut t = OffsetFetchRequestTopic::default();
    t.name = wire_name("t");
    t.partition_indexes = vec![0, 1];
    let mut req = OffsetFetchRequest::default();
    req.group_id = GroupId(StrBytes::from_static_str("g"));
    req.topics = Some(vec![t]);
    let Response::OffsetFetch(r) = rig.ask(version, Request::OffsetFetch(req)).await else {
        panic!()
    };
    r.topics[0]
        .partitions
        .iter()
        .map(|p| (p.partition_index, p.error_code, p.committed_offset))
        .collect()
}

#[tokio::test]
async fn offset_commit_v0_is_zookeepers_apart_from_the_rest_and_kafka_3_refuses_it() {
    let rig = kafka(2).await;
    let Response::OffsetCommit(r) = rig.ask(0, Request::OffsetCommit(commit(0, 42))).await else {
        panic!()
    };
    assert_eq!(r.topics[0].partitions[0].error_code, 0);
    rig.ask(2, Request::OffsetCommit(commit(1, 7))).await;
    assert_eq!(
        committed(&rig, 0).await,
        [(0, 0, 42), (1, UNKNOWN_TOPIC_OR_PARTITION, -1)]
    );
    assert_eq!(committed(&rig, 1).await, [(0, 0, -1), (1, 0, 7)]);
    let three = kafka(3).await;
    let Response::OffsetCommit(r) = three.ask(0, Request::OffsetCommit(commit(0, 42))).await else {
        panic!()
    };
    assert_eq!(r.topics[0].partitions[0].error_code, UNSUPPORTED_VERSION);
    assert!(
        committed(&three, 0)
            .await
            .iter()
            .all(|p| p.1 == UNSUPPORTED_VERSION)
    );
}

#[tokio::test]
async fn kafka_3_9_lists_the_consumer_group_heartbeat_and_refuses_it() {
    let rig = kafka(3).await;
    let mut req = ConsumerGroupHeartbeatRequest::default();
    req.group_id = GroupId(StrBytes::from_static_str("g"));
    let Response::ConsumerGroupHeartbeat(r) =
        rig.ask(0, Request::ConsumerGroupHeartbeat(req)).await
    else {
        panic!()
    };
    assert_eq!(r.error_code, UNSUPPORTED_VERSION);
}

#[tokio::test]
async fn the_oldest_admin_versions_are_written_at_every_older_profile() {
    for major in [1, 2, 3] {
        let rig = rig_with(|c| c.kafka_major = KafkaMajor::new(major).unwrap());
        let mut create = kafka_wire::messages::CreateTopicsRequest::default();
        create.topics = vec![topic("a", 1), topic("b", -5)];
        for v in [0, 1] {
            // (Rig::ask holds the answer to being writable at the version.)
            rig.ask(v, Request::CreateTopics(create.clone())).await;
        }
        let mut r =
            kafka_wire::messages::describe_configs_request::DescribeConfigsResource::default();
        r.resource_type = 2;
        r.resource_name = StrBytes::from_static_str("a");
        let mut describe = kafka_wire::messages::DescribeConfigsRequest::default();
        describe.resources.push(r);
        let Response::DescribeConfigs(d) = rig.ask(0, Request::DescribeConfigs(describe)).await
        else {
            panic!()
        };
        // Kafka 1.1 says which configs are defaults at v0; 2.8 and 3.9 say it of none.
        let defaults = d
            .results
            .iter()
            .flat_map(|r| &r.configs)
            .filter(|c| c.is_default)
            .count();
        assert_eq!(defaults > 0, major == 1, "Kafka {major}");
        let mut delete = kafka_wire::messages::DeleteTopicsRequest::default();
        delete.topic_names = vec![wire_name("a"), wire_name("nothing")];
        rig.ask(0, Request::DeleteTopics(delete)).await;
    }
}
