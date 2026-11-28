//! How many allocations the recorder makes per request, counted by a global allocator that lives in this test binary only.
//!
//! Ignored by default (it is a measurement, not a check): run it with
//! `cargo test -p kafka-console --test recorder_allocations -- --ignored --nocapture`. It plays 100,000 synthetic requests
//! through the recorder as the server does: produces and fetches on eight connections, each naming two topics of twelve
//! partitions (so 24 results, past the 16 a row keeps), the fetches naming their topics by id as v13 does, and a Metadata
//! answer every hundred requests that the recorder learns the ids' names from. docs/port/weight.md has the numbers.

use std::alloc::System;
use std::time::Duration;

use kafka_auth::Principal;
use kafka_console::Recorder;
use kafka_server::{Answer, Asked, Call, ConnId, Observed, Observer, Opened};
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::*;
use kafka_wire::{StrBytes, Uuid};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const REQUESTS: u64 = 100_000;
const TOPICS: [&str; 2] = ["orders.placed", "payments.settled"];

fn id_of(i: usize) -> Uuid {
    Uuid::from_u128(0x1234_5678_9abc_def0_0000_0000_0000_0000 + i as u128)
}

fn produce() -> (Request, Response) {
    let batch = kafka_wire::batch::build::BatchBuilder::new()
        .records(10)
        .build();
    let mut q = ProduceRequest::default();
    q.acks = -1;
    let mut a = ProduceResponse::default();
    for name in TOPICS {
        let mut t = produce_request::TopicProduceData::default();
        t.name = TopicName(StrBytes::from_static_str(name));
        let mut tr = produce_response::TopicProduceResponse::default();
        tr.name = t.name.clone();
        for index in 0..12 {
            let mut p = produce_request::PartitionProduceData::default();
            p.index = index;
            p.records = Some(batch.clone());
            t.partition_data.push(p);
            let mut pr = produce_response::PartitionProduceResponse::default();
            pr.index = index;
            pr.base_offset = 100;
            tr.partition_responses.push(pr);
        }
        q.topic_data.push(t);
        a.responses.push(tr);
    }
    (Request::Produce(q), Response::Produce(a))
}

fn fetch() -> (Request, Response) {
    let batch = kafka_wire::batch::build::BatchBuilder::new()
        .records(10)
        .build();
    let mut q = FetchRequest::default();
    q.max_wait_ms = 500;
    let mut a = FetchResponse::default();
    for i in 0..TOPICS.len() {
        let mut t = fetch_request::FetchTopic::default();
        t.topic_id = id_of(i);
        let mut tr = fetch_response::FetchableTopicResponse::default();
        tr.topic_id = id_of(i);
        for partition in 0..12 {
            let mut p = fetch_request::FetchPartition::default();
            p.partition = partition;
            p.fetch_offset = 100;
            t.partitions.push(p);
            let mut pr = fetch_response::PartitionData::default();
            pr.partition_index = partition;
            pr.high_watermark = 110;
            pr.records = Some(batch.clone());
            tr.partitions.push(pr);
        }
        q.topics.push(t);
        a.responses.push(tr);
    }
    (Request::Fetch(q), Response::Fetch(a))
}

fn metadata() -> (Request, Response) {
    let mut a = MetadataResponse::default();
    for (i, name) in TOPICS.iter().enumerate() {
        let mut t = metadata_response::MetadataResponseTopic::default();
        t.name = Some(TopicName(StrBytes::from_static_str(name)));
        t.topic_id = id_of(i);
        a.topics.push(t);
    }
    (
        Request::Metadata(MetadataRequest::default()),
        Response::Metadata(a),
    )
}

#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn allocations_per_recorded_request() {
    let rec = Recorder::new();
    let who = Principal::anonymous();
    let calls = [produce(), fetch(), metadata()];
    let heads: Vec<RequestHead> = [
        (ApiKey::Produce, 11),
        (ApiKey::Fetch, 13),
        (ApiKey::Metadata, 12),
    ]
    .into_iter()
    .map(|(key, version)| RequestHead {
        key,
        version,
        correlation_id: 1,
        client_id: Some(StrBytes::from_static_str("bench")),
    })
    .collect();
    for conn in 0..8 {
        rec.opened(&Opened {
            id: ConnId(conn),
            peer: Some("127.0.0.1:50402".parse().unwrap()),
        });
    }
    let region = Region::new(GLOBAL);
    for i in 0..REQUESTS {
        let which = if i % 100 == 0 {
            2
        } else {
            usize::from(i % 2 == 1)
        };
        let (request, response) = &calls[which];
        let pending = rec.begin(&Call {
            conn: ConnId(i % 8),
            principal: &who,
            head: &heads[which],
            asked: Asked::Request(request),
            request_bytes: 4096,
        });
        rec.end(
            pending,
            &Observed {
                answer: Answer::Response(response),
                took: Duration::from_micros(300),
                response_bytes: 4096,
                fault: None,
            },
        );
    }
    let stats = region.change();
    let made = stats.allocations + stats.reallocations;
    #[expect(clippy::cast_precision_loss, reason = "a printed ratio")]
    let per = made as f64 / REQUESTS as f64;
    println!(
        "recorder: {made} allocations ({} bytes) for {REQUESTS} requests, {per:.2} per request",
        stats.bytes_allocated
    );
    assert!(rec.held().0 > 0, "the requests were recorded");
}
