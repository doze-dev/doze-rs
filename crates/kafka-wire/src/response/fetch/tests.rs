//! The envelope against kafka-protocol: the same bytes for every Fetch response, at every version this path writes, however
//! many of its records are shared; and the shared records really are the log's buffers, not copies of them.

use std::collections::BTreeMap;

use bytes::Buf;
use kafka_protocol::messages::ResponseHeader;
use kafka_protocol::messages::fetch_response::AbortedTransaction;
use kafka_protocol::messages::{BrokerId, ProducerId, TopicName};
use proptest::prelude::*;
use uuid::Uuid;

use super::*;
use crate::api::Response;
use crate::response::encode;

/// What kafka-protocol writes (the encoder this path replaces), and what this path writes, laid end to end.
fn both(version: i16, fetch: &FetchResponse, shared_from: usize) -> (Bytes, Bytes, Frame) {
    let old = encode(42, version, &Response::Fetch(fetch.clone())).unwrap();
    let new = frame(42, version, fetch, shared_from).unwrap();
    let mut copy = new.clone();
    (old, copy.copy_to_bytes(copy.remaining()), new)
}

/// Checks the two agree, and that every record buffer of `shared_from` bytes or more is a piece of the frame itself.
fn agrees(version: i16, fetch: &FetchResponse, shared_from: usize) {
    let (old, new, frame) = both(version, fetch, shared_from);
    assert_eq!(old, new, "v{version}, shared from {shared_from}");
    let mut slots = vec![std::io::IoSlice::new(&[]); frame.pieces()];
    assert_eq!(frame.chunks_vectored(&mut slots), frame.pieces());
    let pieces: Vec<*const u8> = slots.iter().map(|s| s.as_ptr()).collect();
    let mut shared = 0;
    for records in fetch
        .responses
        .iter()
        .flat_map(|t| &t.partitions)
        .filter_map(|p| p.records.as_ref())
        .filter(|r| !r.is_empty() && r.len() >= shared_from)
    {
        assert!(
            pieces.contains(&records.as_ptr()),
            "v{version}: records copied"
        );
        shared += 1;
    }
    // The envelope is cut once around each shared buffer; it may end on records (no tagged fields below v12).
    assert!(frame.pieces() <= 1 + 2 * shared, "v{version}");
}

fn records(len: usize, fill: u8) -> Bytes {
    Bytes::from(vec![fill; len])
}

fn txn(producer: i64, first: i64) -> AbortedTransaction {
    let mut t = AbortedTransaction::default();
    t.producer_id = ProducerId(producer);
    t.first_offset = first;
    t
}

/// A partition with every field this version has set to something other than its default.
fn partition(version: i16, index: i32, records: Option<Bytes>) -> PartitionData {
    let mut p = PartitionData::default();
    p.partition_index = index;
    p.error_code = 3;
    p.high_watermark = 1_000_000 + i64::from(index);
    p.last_stable_offset = 999_999;
    p.log_start_offset = 17;
    p.aborted_transactions = Some(vec![txn(7, 100), txn(8, 2_000)]);
    if version >= 11 {
        p.preferred_read_replica = BrokerId(1);
    }
    p.records = records;
    if version >= FLEXIBLE {
        p.diverging_epoch.epoch = 5;
        p.diverging_epoch.end_offset = 900;
        p.current_leader.leader_id = BrokerId(1);
        p.current_leader.leader_epoch = 9;
        p.snapshot_id.end_offset = 12;
        p.snapshot_id.epoch = 2;
        p.unknown_tagged_fields = BTreeMap::from([(7, Bytes::from_static(b"seven"))]);
    }
    p
}

fn topic(name: &str, partitions: Vec<PartitionData>) -> FetchableTopicResponse {
    let mut t = FetchableTopicResponse::default();
    t.topic = TopicName(StrBytes::from_string(name.to_owned()));
    t.topic_id = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
    t.partitions = partitions;
    t
}

/// A response of two topics: one with records of every kind (absent, empty, small, at the threshold, large, and a buffer
/// shared by two partitions), one with no partitions.
fn mixed(version: i16) -> FetchResponse {
    let big = records(64 << 10, 0xab);
    let parts = vec![
        partition(version, 0, None),
        partition(version, 1, Some(Bytes::new())),
        partition(version, 2, Some(records(100, 1))),
        partition(version, 3, Some(records(SHARED_FROM - 1, 2))),
        partition(version, 4, Some(records(SHARED_FROM, 3))),
        partition(version, 5, Some(big.clone())),
        partition(version, 6, Some(big)),
    ];
    let mut r = FetchResponse::default();
    r.throttle_time_ms = 250;
    if version >= 7 {
        r.error_code = 0;
        r.session_id = 77;
    }
    r.responses = vec![topic("orders", parts), topic("empty", Vec::new())];
    if version >= FLEXIBLE {
        r.unknown_tagged_fields = BTreeMap::from([(1, Bytes::from_static(b"x"))]);
    }
    r
}

const SHARINGS: [usize; 4] = [0, 1, SHARED_FROM, usize::MAX];

#[test]
fn every_version_writes_what_kafka_protocol_writes() {
    for version in VERSIONS {
        for from in SHARINGS {
            agrees(version, &mixed(version), from);
            agrees(version, &FetchResponse::default(), from);
        }
    }
}

#[test]
fn a_fetch_that_ends_on_its_records_has_no_empty_last_piece() {
    // Below v12 nothing follows a partition's records: the frame's last byte is the last record's.
    for version in 4..FLEXIBLE {
        let mut r = FetchResponse::default();
        r.responses = vec![topic(
            "t",
            vec![partition(version, 0, Some(records(1 << 20, 9)))],
        )];
        let (_, _, frame) = both(version, &r, SHARED_FROM);
        assert_eq!(frame.pieces(), 2, "v{version}");
        agrees(version, &r, SHARED_FROM);
    }
}

#[test]
fn null_and_empty_aborted_lists() {
    for version in VERSIONS {
        let mut none = partition(version, 0, Some(records(SHARED_FROM, 1)));
        none.aborted_transactions = None;
        let mut empty = partition(version, 1, Some(records(SHARED_FROM, 2)));
        empty.aborted_transactions = Some(Vec::new());
        let mut r = FetchResponse::default();
        r.responses = vec![topic("t", vec![none, empty])];
        for from in SHARINGS {
            agrees(version, &r, from);
        }
    }
}

#[test]
fn an_unknown_tag_where_a_known_one_belongs_is_refused_not_written_short() {
    // kafka-protocol counts such a tag (and its size) but does not write it, so its frame names a tag it does not hold and is
    // shorter than kafka-protocol's own count of it. The broker sets no unknown tag; were one ever set there, this path
    // refuses the response (the frame's length is checked against that count) rather than write a frame no client can read.
    for version in FLEXIBLE..=*VERSIONS.end() {
        let mut hidden_in_partition = FetchResponse::default();
        let mut p = partition(version, 0, None);
        p.unknown_tagged_fields
            .insert(1, Bytes::from_static(b"hidden"));
        hidden_in_partition.responses = vec![topic("t", vec![p])];
        let mut hidden_at_top = FetchResponse::default();
        hidden_at_top
            .unknown_tagged_fields
            .insert(0, Bytes::from_static(b"top"));
        for r in [hidden_in_partition, hidden_at_top] {
            let response = Response::Fetch(r.clone());
            let old = encode(1, version, &response).unwrap();
            let header = ResponseHeader::default()
                .compute_size(ApiKey::Fetch.response_header_version(version))
                .unwrap();
            assert!(old.len() < 4 + header + response.compute_size(version).unwrap());
            assert!(frame(1, version, &r, SHARED_FROM).is_err(), "v{version}");
        }
    }
}

#[test]
fn a_field_set_at_a_version_without_it_is_refused_as_kafka_protocol_refuses_it() {
    let mut early_session = FetchResponse::default();
    early_session.session_id = 1;
    let mut early_replica = FetchResponse::default();
    early_replica.responses = vec![topic("t", vec![partition(11, 0, None)])];
    for (version, r) in [(6, early_session), (10, early_replica)] {
        let old = encode(1, version, &Response::Fetch(r.clone())).unwrap_err();
        let new = frame(1, version, &r, SHARED_FROM).unwrap_err();
        assert_eq!(old, new, "v{version}");
    }
}

fn any_records() -> impl Strategy<Value = Option<Bytes>> {
    prop_oneof![
        Just(None),
        Just(Some(Bytes::new())),
        (1_usize..300).prop_map(|n| Some(records(n, 5))),
        (SHARED_FROM - 2..SHARED_FROM + 2).prop_map(|n| Some(records(n, 6))),
        (10_000_usize..70_000).prop_map(|n| Some(records(n, 7))),
    ]
}

fn any_partition(version: i16) -> impl Strategy<Value = PartitionData> {
    (
        any_records(),
        any::<i32>(),
        proptest::option::of(proptest::collection::vec(
            (any::<i64>(), any::<i64>()),
            0..3,
        )),
        any::<bool>(),
        proptest::collection::btree_map(
            3_i32..200,
            proptest::collection::vec(any::<u8>(), 0..20),
            0..3,
        ),
    )
        .prop_map(move |(records, index, aborted, tagged, unknown)| {
            let mut p = partition(version, index, records);
            p.aborted_transactions =
                aborted.map(|a| a.into_iter().map(|(id, first)| txn(id, first)).collect());
            if version >= FLEXIBLE && !tagged {
                p.diverging_epoch = EpochEndOffset::default();
                p.snapshot_id = SnapshotId::default();
            }
            if version >= FLEXIBLE {
                p.unknown_tagged_fields = unknown
                    .into_iter()
                    .map(|(k, v)| (k, Bytes::from(v)))
                    .collect();
            }
            p
        })
}

fn any_fetch() -> impl Strategy<Value = (i16, FetchResponse)> {
    VERSIONS.prop_flat_map(|version| {
        let topics = proptest::collection::vec(
            (
                "[a-z]{0,20}",
                proptest::collection::vec(any_partition(version), 0..4),
            ),
            0..4,
        );
        (Just(version), topics, any::<i32>()).prop_map(|(version, topics, throttle)| {
            let mut r = FetchResponse::default();
            r.throttle_time_ms = throttle;
            r.responses = topics
                .into_iter()
                .map(|(name, ps)| topic(&name, ps))
                .collect();
            (version, r)
        })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn any_fetch_response_is_written_as_kafka_protocol_writes_it((version, r) in any_fetch()) {
        for from in [0, SHARED_FROM] {
            agrees(version, &r, from);
        }
    }
}
