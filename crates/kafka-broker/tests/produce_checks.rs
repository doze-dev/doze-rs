//! What a Produce is held to before it is stored: the request's shape, a batch's header and size, transactions,
//! topic ids, and the clock.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use bytes::Bytes;
use kafka_wire::api::Response;
use kafka_wire::batch::build::BatchBuilder;
use support::{produce_request, records, rig, stored, topic, with_config};

#[tokio::test]
async fn a_request_the_broker_cannot_take_apart_is_refused_on_every_partition_and_by_the_first_fault()
 {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let mut corrupt = records(1).to_vec();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 0xff;
    // (what was sent, the code, and the log start offset Kafka answers with: -1 when it refuses before it has asked
    // the partition anything.)
    let two = Bytes::from([records(1), records(1)].concat());
    let cases: Vec<(&str, i16, Bytes, i16, i64)> = vec![
        ("acks 2", 2, records(1), 21, -1),
        ("bad checksum", 1, Bytes::from(corrupt), 2, -1),
        ("nothing whole", 1, Bytes::from(vec![0; 10]), 87, -1),
        ("two batches", 1, two, 87, -1),
        (
            "base offset 7",
            1,
            BatchBuilder::new().base_offset(7).build(),
            87,
            0,
        ),
        (
            "a producer id without a sequence",
            1,
            BatchBuilder::new().producer(5, 0, -1).build(),
            87,
            0,
        ),
        (
            "a control batch",
            1,
            BatchBuilder::new().attributes(0x20).build(),
            87,
            0,
        ),
    ];
    for (label, acks, batch, code, start) in cases {
        let r = rig.produce(acks, "t", 0, batch).await;
        assert_eq!(
            (r.error_code, r.base_offset, r.log_start_offset),
            (code, -1, start),
            "{label}"
        );
    }
    // Nothing of that was stored, and the partition is as it was.
    assert_eq!(rig.produce(1, "t", 0, records(1)).await.base_offset, 0);
}

#[tokio::test]
async fn a_batch_the_first_look_cannot_make_sense_of_fails_the_other_partitions_too() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    // Kafka looks at every partition's first batch before it handles any.
    let nonsense = Bytes::from(vec![
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 9, 0, 0,
    ]);
    let req = produce_request(1, "t", &[(0, records(1)), (1, nonsense)]);
    let Response::Produce(r) = rig.ask(9, kafka_wire::api::Request::Produce(req)).await else {
        panic!()
    };
    let codes: Vec<_> = r.responses[0]
        .partition_responses
        .iter()
        .map(|p| p.error_code)
        .collect();
    assert_eq!(codes, [2, 2]);
    assert_eq!(
        r.responses[0].partition_responses[0]
            .error_message
            .as_deref(),
        Some("Record size 3 is less than the minimum record overhead (14)")
    );
    assert_eq!(stored(&rig.fs, "t", 0).len(), 0);
}

#[tokio::test]
async fn the_topic_limits_how_big_a_batch_may_be_and_zstd_needs_v7() {
    let rig = rig();
    rig.create(
        vec![with_config(topic("small", 1), "max.message.bytes", "100")],
        false,
    )
    .await;
    let big = BatchBuilder::new().records(1).body(vec![0; 200]).build();
    let r = rig.produce(1, "small", 0, big).await;
    assert_eq!((r.error_code, r.base_offset), (10, -1));
    rig.create(vec![topic("t", 1)], false).await;
    let zstd = BatchBuilder::new().attributes(4).build();
    let req = produce_request(1, "t", &[(0, zstd)]);
    let Response::Produce(r) = rig.ask(6, kafka_wire::api::Request::Produce(req)).await else {
        panic!()
    };
    assert_eq!(r.responses[0].partition_responses[0].error_code, 76);
}

#[tokio::test]
async fn a_transactional_batch_needs_a_transactional_id_and_transactions_are_not_served_yet() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let transactional = BatchBuilder::new()
        .attributes(0x10)
        .producer(5, 0, 0)
        .build();
    // No transactional id: the whole request is refused, every partition, as Kafka does.
    let req = produce_request(1, "t", &[(0, records(1)), (1, transactional.clone())]);
    let Response::Produce(r) = rig.ask(9, kafka_wire::api::Request::Produce(req)).await else {
        panic!()
    };
    let codes: Vec<_> = r.responses[0]
        .partition_responses
        .iter()
        .map(|p| p.error_code)
        .collect();
    assert_eq!(codes, [53, 53]);
    // With one, it is refused for the partition alone until Phase 6 has transactions.
    let mut req = produce_request(1, "t", &[(0, records(1)), (1, transactional)]);
    req.transactional_id = Some(kafka_wire::messages::TransactionalId(
        kafka_wire::StrBytes::from_static_str("x"),
    ));
    let Response::Produce(r) = rig.ask(9, kafka_wire::api::Request::Produce(req)).await else {
        panic!()
    };
    let codes: Vec<_> = r.responses[0]
        .partition_responses
        .iter()
        .map(|p| p.error_code)
        .collect();
    assert_eq!(codes, [0, 48]);
}

#[tokio::test]
async fn from_v13_a_topic_is_named_by_its_id_and_the_answer_says_which() {
    let rig = rig();
    let made = rig.create(vec![topic("t", 1)], false).await;
    let mut req = produce_request(1, "", &[(0, records(1))]);
    req.topic_data[0].topic_id = made[0].topic_id;
    let Response::Produce(r) = rig.ask(13, kafka_wire::api::Request::Produce(req)).await else {
        panic!()
    };
    assert_eq!(r.responses[0].topic_id, made[0].topic_id);
    let first = &r.responses[0].partition_responses[0];
    assert_eq!((first.error_code, first.base_offset), (0, 0));
    // An id that is no topic's is unknown, whatever name the request carries.
    let mut req = produce_request(1, "t", &[(0, records(1))]);
    req.topic_data[0].topic_id = kafka_wire::Uuid::from_u128(9);
    let Response::Produce(r) = rig.ask(13, kafka_wire::api::Request::Produce(req)).await else {
        panic!()
    };
    assert_eq!(r.responses[0].partition_responses[0].error_code, 3);
}

#[tokio::test]
async fn a_topic_that_stamps_with_the_brokers_clock_says_when() {
    let rig = rig();
    let stamping = with_config(topic("t", 1), "message.timestamp.type", "LogAppendTime");
    rig.create(vec![stamping, topic("plain", 1)], false).await;
    let r = rig.produce(-1, "t", 0, records(1)).await;
    assert!(
        r.log_append_time_ms > 1_700_000_000_000,
        "{}",
        r.log_append_time_ms
    );
    let held = stored(&rig.fs, "t", 0);
    let peek = kafka_wire::batch::Peek::read(&held[0]).unwrap();
    assert_eq!(
        peek.max_timestamp.get(),
        r.log_append_time_ms,
        "the batch carries the time it was answered with"
    );
    assert_eq!(
        rig.produce(1, "plain", 0, records(1))
            .await
            .log_append_time_ms,
        -1
    );
}
