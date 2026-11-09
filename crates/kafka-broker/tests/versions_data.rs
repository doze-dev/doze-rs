//! Every version of the topic and record APIs the broker serves answers in a form that can be written. (A field set
//! that a version does not have makes the encoder refuse and the connection is closed: real clients met that with
//! FindCoordinator v4 and with ListOffsets below v4, which no test had asked.) The rig encodes every response it is
//! given, so asking is the whole test. The group APIs are in `versions.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_wire::api::Request;
use kafka_wire::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_wire::messages::{
    CreateTopicsRequest, DeleteTopicsRequest, InitProducerIdRequest, ListOffsetsRequest,
};
use support::{fetch_request, produce_request, records, rig, topic, wire_name};

#[tokio::test]
async fn producing_fetching_and_listing_offsets_at_every_version() {
    let rig = rig();
    rig.create(vec![topic("t", 2), topic("empty", 1)], false)
        .await;
    for v in 3..=9 {
        let req = produce_request(1, "t", &[(0, records(2)), (1, records(1))]);
        rig.ask(v, Request::Produce(req)).await;
        // acks=0 is answered by nothing, and a topic that is not there is answered with its error.
        let req = produce_request(-1, "nope", &[(0, records(1))]);
        rig.ask(v, Request::Produce(req)).await;
    }
    for v in 4..=12 {
        let asks = [
            ("t", 0, 0, 1 << 20),
            ("t", 1, 1, 1 << 20),
            ("empty", 0, 0, 1 << 20),
        ];
        rig.ask(v, Request::Fetch(fetch_request(&asks, 0, 1, 1 << 20)))
            .await;
        // Past the end, and a partition that is not there.
        let asks = [
            ("t", 0, 99, 1 << 20),
            ("t", 7, 0, 1 << 20),
            ("nope", 0, 0, 1 << 20),
        ];
        rig.ask(v, Request::Fetch(fetch_request(&asks, 0, 1, 1 << 20)))
            .await;
    }
    for v in 1..=7 {
        // Earliest, latest, a time, and (from v7) the newest record; and the answers for what is not there.
        let mut stamps = vec![-2, -1, 0];
        if v >= 7 {
            stamps.push(-3);
        }
        for topic_name in ["t", "empty", "nope"] {
            for ts in &stamps {
                let mut p = ListOffsetsPartition::default();
                p.partition_index = 0;
                p.timestamp = *ts;
                let mut t = ListOffsetsTopic::default();
                t.name = wire_name(topic_name);
                t.partitions.push(p);
                let mut req = ListOffsetsRequest::default();
                req.topics.push(t);
                rig.ask(v, Request::ListOffsets(req)).await;
            }
        }
    }
}

#[tokio::test]
async fn metadata_topics_and_producer_ids_at_every_version() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    for v in 0..=12 {
        rig.metadata(v, None, false).await;
        rig.metadata(v, Some(&["t", "nope"]), false).await;
        rig.metadata(v, Some(&[]), false).await;
    }
    for v in 2..=7 {
        let mut req = CreateTopicsRequest::default();
        // One that is made, one that is refused, one that already exists.
        req.topics = vec![
            topic(&format!("made-{v}"), 1),
            topic("t", 1),
            topic("a b", 1),
        ];
        rig.ask(v, Request::CreateTopics(req)).await;
    }
    for v in 1..=5 {
        let mut req = DeleteTopicsRequest::default();
        req.topic_names = vec![wire_name(&format!("made-{}", v + 1)), wire_name("nope")];
        rig.ask(v, Request::DeleteTopics(req)).await;
    }
    for v in 0..=4 {
        let mut req = InitProducerIdRequest::default();
        req.transaction_timeout_ms = -1;
        rig.ask(v, Request::InitProducerId(req)).await;
    }
}
