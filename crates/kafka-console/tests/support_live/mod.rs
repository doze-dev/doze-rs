//! A raw Kafka client over a real connection, the larder's traffic, and a world whose server has the recorder installed: shared
//! by the tests of the recorder and Live (`mod support_write; mod support_live;`).
#![allow(
    dead_code,
    unreachable_pub,
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::sync::Arc;

use axum::Router;
use bytes::Bytes;
use kafka_console::Recorder;
use kafka_console::base::Base;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_protocol::protocol::Encodable;
use kafka_wire::StrBytes;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::*;
use kafka_wire::testing::{framed, request_payload};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::support_write::{World, world};

pub struct Wire {
    stream: TcpStream,
    next: i32,
}

impl Wire {
    pub async fn connect(w: &World) -> Self {
        let addr = w.server.as_ref().unwrap().local_addr();
        Self {
            stream: TcpStream::connect(addr).await.unwrap(),
            next: 1,
        }
    }

    /// One request and its whole answer (the payload after the length).
    pub async fn call<R: Encodable>(
        &mut self,
        key: ApiKey,
        version: i16,
        client: &str,
        body: &R,
    ) -> Bytes {
        let id = self.next;
        self.next += 1;
        self.stream
            .write_all(&framed(&request_payload(
                key,
                version,
                id,
                Some(client),
                body,
            )))
            .await
            .unwrap();
        let mut len = [0; 4];
        self.stream.read_exact(&mut len).await.unwrap();
        let mut payload = vec![0; u32::from_be_bytes(len) as usize];
        self.stream.read_exact(&mut payload).await.unwrap();
        Bytes::from(payload)
    }
}

pub fn s(text: &str) -> StrBytes {
    StrBytes::from_string(text.to_owned())
}

pub fn versions(software: &str) -> ApiVersionsRequest {
    let mut r = ApiVersionsRequest::default();
    r.client_software_name = s(software);
    r.client_software_version = s("2.3.0");
    r
}

pub fn produce(topic: &str, batch: Bytes, txn: Option<&str>) -> ProduceRequest {
    let mut p = PartitionProduceData::default();
    p.index = 0;
    p.records = Some(batch);
    let mut t = TopicProduceData::default();
    t.name = TopicName(s(topic));
    t.partition_data = vec![p];
    let mut r = ProduceRequest::default();
    r.transactional_id = txn.map(|t| TransactionalId(s(t)));
    r.acks = -1;
    r.timeout_ms = 5000;
    r.topic_data = vec![t];
    r
}

pub fn batch(key: Option<&str>, value: Vec<u8>, producer: Option<(i64, i16)>) -> Bytes {
    let rec = Rec {
        key: key.map(|k| k.as_bytes().to_vec()),
        value: Some(value),
        ..Rec::default()
    };
    let mut b = BatchBuilder::from_records(1_700_000_000_000, &[rec]).last_offset_delta(0);
    if let Some((id, epoch)) = producer {
        b = b.producer(id, epoch, 0).attributes(0x10);
    }
    b.build()
}

pub fn fetch(topic: &str) -> FetchRequest {
    let mut p = FetchPartition::default();
    p.partition = 0;
    p.fetch_offset = 0;
    p.partition_max_bytes = 1 << 20;
    let mut t = FetchTopic::default();
    t.topic = TopicName(s(topic));
    t.partitions = vec![p];
    let mut r = FetchRequest::default();
    r.max_wait_ms = 0;
    r.max_bytes = 1 << 20;
    r.topics = vec![t];
    r
}

pub fn init(txn: &str) -> InitProducerIdRequest {
    let mut r = InitProducerIdRequest::default();
    r.transactional_id = Some(TransactionalId(s(txn)));
    r.transaction_timeout_ms = 60_000;
    r
}

pub fn router(w: &World, recorder: &Arc<Recorder>) -> Router {
    let live = Live::new(w.backend.clone(), w.facts.clone()).with_recorder(Arc::clone(recorder));
    let state = State::new(
        Source::Live(Box::new(live)),
        Base::root(),
        vec![],
        CancellationToken::new(),
    );
    kafka_console::app::router(state)
}

/// A world whose server has the recorder installed.
pub async fn recorded(name: &str, deny: bool) -> (World, Arc<Recorder>) {
    let recorder = Recorder::new();
    let r = Arc::clone(&recorder);
    let w = world(name, move |bc, sc| {
        sc.observer = Some(r);
        bc.acl_default_allow = !deny;
    })
    .await;
    (w, recorder)
}

/// The traffic of the larder: a Python consumer, a Go transactional producer, an admin tool that is refused, and a hostile name.
pub async fn traffic(w: &World) {
    w.create_topic("orders", 1, &[]).await;
    w.create_topic("inventory.levels", 1, &[("cleanup.policy", "compact")])
        .await;
    let mut py = Wire::connect(w).await;
    py.call(
        ApiKey::ApiVersions,
        3,
        "fulfilment",
        &versions("confluent-kafka-python"),
    )
    .await;
    py.call(
        ApiKey::Metadata,
        12,
        "fulfilment",
        &MetadataRequest::default(),
    )
    .await;
    for _ in 0..25 {
        py.call(ApiKey::Fetch, 12, "fulfilment", &fetch("orders"))
            .await;
    }
    let mut go = Wire::connect(w).await;
    go.call(ApiKey::ApiVersions, 3, "orders", &versions("sarama"))
        .await;
    let first = go
        .call(ApiKey::InitProducerId, 4, "orders", &init("orders-eos"))
        .await;
    go.call(ApiKey::InitProducerId, 4, "orders", &init("orders-eos"))
        .await;
    // The first epoch (0) is fenced now: a produce that carries it is refused. The answer is: correlation id, a tagged-fields
    // byte (header v1), throttle (4), error (2), then the producer id (8) and epoch (2).
    let pid = i64::from_be_bytes(first[11..19].try_into().unwrap());
    go.call(
        ApiKey::Produce,
        9,
        "orders",
        &produce(
            "orders",
            batch(Some("k"), b"x".to_vec(), Some((pid, 0))),
            Some("orders-eos"),
        ),
    )
    .await;
    let mut cli = Wire::connect(w).await;
    cli.call(ApiKey::ApiVersions, 3, "admin-cli", &versions("librdkafka"))
        .await;
    cli.call(
        ApiKey::Produce,
        9,
        "admin-cli",
        &produce(
            "inventory.levels",
            batch(None, b"FV-TOM-V:12".to_vec(), None),
            None,
        ),
    )
    .await;
    cli.call(
        ApiKey::Produce,
        9,
        "admin-cli",
        &produce(
            "orders",
            batch(Some("k"), vec![b'z'; 1_300_000], None),
            None,
        ),
    )
    .await;
    cli.call(
        ApiKey::Produce,
        9,
        "admin-cli",
        &produce("orders", batch(Some("k"), b"hello".to_vec(), None), None),
    )
    .await;
    let mut bad = Wire::connect(w).await;
    bad.call(
        ApiKey::ApiVersions,
        3,
        "<script>alert(1)</script>",
        &versions("<img src=x onerror=alert(2)>"),
    )
    .await;
}
