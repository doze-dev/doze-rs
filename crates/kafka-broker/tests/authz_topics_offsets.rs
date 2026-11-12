//! ListOffsets, DeleteRecords and DescribeProducers under ACLs (slice 8d-1): the shape of each denial, the order of the answers
//! and what is hidden. Every row is from the Kafka 4.3.1 source, not from a recording: `KafkaApis.scala`
//! `handleListOffsetRequest` (781-821), `handleDeleteRecordsRequest` (1538-1601), `handleDescribeProducersRequest` (2485-2529).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use support::Rig;
use support::acls::{CREATE, DESCRIBE, READ, WRITE, admin, user};
use topics::*;

const ALTER: i8 = 7;

// ListOffsets

#[tokio::test]
async fn list_offsets_puts_the_refused_topics_after_the_others_with_no_offset() {
    let rig = rig_with_topics(&["a", "b"]).await;
    grant(&rig, vec![allow("a", "User:alice", DESCRIBE)]).await;
    for version in 1..=7 {
        let r = rig
            .list_offsets_as(&user("alice"), version, &[("b", &[0, 1]), ("a", &[0])])
            .await;
        let shape: Vec<_> = r
            .topics
            .iter()
            .map(|t| {
                (
                    t.name.0.to_string(),
                    t.partitions
                        .iter()
                        .map(|p| p.error_code)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [("a".into(), vec![0]), ("b".into(), vec![29, 29])],
            "v{version}: the allowed topic first (KA:805-807)"
        );
        let denied = &r.topics[1].partitions[0];
        assert_eq!((denied.timestamp, denied.offset), (-1, -1));
        assert_eq!(r.topics[0].partitions[0].offset, 1, "a is answered in full");
    }
}

#[tokio::test]
async fn list_offsets_is_granted_by_any_right_that_implies_describe_and_denied_only_by_describe() {
    for (op, answer) in [
        (READ, 0),
        (WRITE, 0),
        (DELETE, 0),
        (ALTER, 0),
        (DESCRIBE, 0),
        (CREATE, 29),
        (ALTER_CONFIGS, 29),
    ] {
        let rig = rig_with_topics(&["a"]).await;
        grant(&rig, vec![allow("a", "User:alice", op)]).await;
        let r = rig.list_offsets_as(&user("alice"), 7, &[("a", &[0])]).await;
        assert_eq!(r.topics[0].partitions[0].error_code, answer, "ALLOW {op}");
    }
    let rig = rig_with_topics(&["a"]).await;
    grant(
        &rig,
        vec![
            allow("a", "User:alice", ALTER),
            deny("a", "User:alice", READ),
        ],
    )
    .await;
    let r = rig.list_offsets_as(&user("alice"), 7, &[("a", &[0])]).await;
    assert_eq!(
        r.topics[0].partitions[0].error_code, 0,
        "DENY READ is not DENY DESCRIBE"
    );
    grant(&rig, vec![deny("a", "User:alice", DESCRIBE)]).await;
    let r = rig.list_offsets_as(&user("alice"), 7, &[("a", &[0])]).await;
    assert_eq!(r.topics[0].partitions[0].error_code, 29);
}

// DeleteRecords

#[tokio::test]
async fn delete_records_refuses_a_topic_per_partition_with_no_low_watermark_and_deletes_nothing() {
    for version in 0..=2 {
        let rig = rig_with_topics(&["a", "b"]).await;
        grant(&rig, vec![allow("a", "User:alice", DELETE)]).await;
        let r = rig
            .delete_records_as(&user("alice"), version, &[("b", &[0, 1]), ("a", &[0])])
            .await;
        let shape: Vec<_> = r
            .topics
            .iter()
            .map(|t| {
                (
                    t.name.0.to_string(),
                    t.partitions
                        .iter()
                        .map(|p| (p.error_code, p.low_watermark))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("b".into(), vec![(29, -1), (29, -1)]),
                ("a".into(), vec![(0, 1)])
            ],
            "v{version}: refused where it is, in the order asked (Kafka's is a map's)"
        );
        // b's record is still there to be read from offset 0; a's was deleted, so its start moved up to 1.
        let still = rig
            .fetch_topics_as(&admin(), 12, &[("b", 0, 0, 1 << 20), ("a", 0, 0, 1 << 20)])
            .await;
        assert_eq!(
            Rig::fetched(&still),
            [("b".into(), 0, 0), ("a".into(), 0, 1)],
            "1 is OFFSET_OUT_OF_RANGE"
        );
    }
}

// DescribeProducers

#[tokio::test]
async fn describe_producers_refuses_each_partition_with_the_error_text_and_judges_the_name_first() {
    let rig = rig_with_topics(&["a"]).await;
    grant(&rig, vec![allow("a", "User:alice", READ)]).await;
    let r = rig
        .describe_producers_as(&user("bob"), &[("a", &[0, 1]), ("ghost", &[0])])
        .await;
    for t in &r.topics {
        for p in &t.partitions {
            assert_eq!(
                p.error_code, 29,
                "{}: bob may not read it, whether it is there or not",
                t.name.0
            );
            assert_eq!(
                p.error_message.as_deref(),
                Some("Topic authorization failed.")
            );
        }
    }
    // A name that cannot be a topic's is INVALID_TOPIC_EXCEPTION, before READ is asked (KA:2503-2505).
    let r = rig
        .describe_producers_as(&user("bob"), &[("bad name", &[0])])
        .await;
    assert_eq!(r.topics[0].partitions[0].error_code, 17);
    // alice may read "a": its producers are told (none yet), and a partition it does not have is unknown.
    let r = rig
        .describe_producers_as(&user("alice"), &[("a", &[0, 5])])
        .await;
    assert_eq!(
        r.topics[0]
            .partitions
            .iter()
            .map(|p| p.error_code)
            .collect::<Vec<_>>(),
        [0, 3]
    );
}
