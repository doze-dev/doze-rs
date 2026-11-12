//! The world the live topic tests run in: a real broker in this process with its Kafka listener, seeded with topics of several
//! shapes, and the console's router over it.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use crate::support::{get, send};
use axum::Router;
use axum::http::StatusCode;
use bytes::Bytes;
use kafka_auth::Principal;
use kafka_broker::{Broker, Caller, Config as BrokerConfig, Listener, Outcome};
use kafka_console::Backend;
use kafka_console::base::Base;
use kafka_console::facts::Facts;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_server::{Config as ServerConfig, Running, Server};
use kafka_storage::fs::RealFs;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::build::{BatchBuilder, Rec, encode_records};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::create_topics_request::{CreatableTopic, CreatableTopicConfig};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    AddPartitionsToTxnRequest, CreateTopicsRequest, InitProducerIdRequest, ProduceRequest,
    ProducerId, TopicName, TransactionalId,
};
use tokio_util::sync::CancellationToken;

pub struct World {
    pub dir: PathBuf,
    pub broker: Arc<Broker<RealFs>>,
    pub server: Option<Running>,
    pub backend: Backend,
    pub facts: Facts,
}

pub async fn world(name: &str, tweak: impl FnOnce(&mut BrokerConfig)) -> World {
    let dir = std::env::temp_dir().join(format!("doze-topics-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut bc = BrokerConfig::new(dir.clone());
    tweak(&mut bc);
    let sc = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        ..ServerConfig::default()
    };
    let broker = Arc::new(Broker::open(RealFs, bc).unwrap());
    let server = Server::bind(sc.clone(), Arc::clone(&broker)).await.unwrap();
    let listener: Listener = sc.advertise(server.local_addr());
    let facts = Facts {
        listen: server.local_addr(),
        advertised_host: listener.advertised_host.clone(),
        advertised_port: u16::try_from(listener.advertised_port).unwrap(),
        tls: false,
        sasl: vec![],
        kafka_major: 4,
        allow_hosts: vec![],
        unprotected: false,
    };
    let backend = Backend::new(Arc::clone(&broker), listener, sc.profile);
    World {
        dir,
        broker,
        server: Some(server),
        backend,
        facts,
    }
}

pub fn now_ms() -> i64 {
    kafka_console::source::now_ms()
}

pub fn router(backend: Backend, facts: Facts) -> Router {
    let state = State::new(
        Source::Live(Box::new(Live::new(backend, facts))),
        Base::root(),
        vec![],
        CancellationToken::new(),
    );
    kafka_console::app::router(state)
}

impl World {
    pub fn router(&self) -> Router {
        router(self.backend.clone(), self.facts.clone())
    }

    pub fn router_as(&self, caller: Caller) -> Router {
        router(self.backend.clone().acting_as(caller), self.facts.clone())
    }

    pub async fn ask(&self, version: i16, request: Request) -> Response {
        let head = RequestHead {
            key: request.api_key(),
            version,
            correlation_id: 1,
            client_id: Some(StrBytes::from_static_str("seed")),
        };
        let caller = Caller::new(Principal::console(), Some("127.0.0.1".parse().unwrap()));
        let listener = Listener {
            advertised_host: "127.0.0.1".into(),
            advertised_port: 1,
            client_host: "/127.0.0.1".into(),
        };
        match self.broker.handle(&listener, &caller, &head, request).await {
            Outcome::Respond(r) => r,
            other => panic!("{other:?}"),
        }
    }

    pub async fn create(&self, name: &str, partitions: i32, configs: &[(&str, &str)]) {
        let mut t = CreatableTopic::default();
        t.name = TopicName(StrBytes::from_string(name.to_owned()));
        t.num_partitions = partitions;
        t.replication_factor = 1;
        for (k, v) in configs {
            let mut c = CreatableTopicConfig::default();
            c.name = StrBytes::from_string((*k).to_owned());
            c.value = Some(StrBytes::from_string((*v).to_owned()));
            t.configs.push(c);
        }
        let mut r = CreateTopicsRequest::default();
        r.topics = vec![t];
        let Response::CreateTopics(r) = self.ask(7, Request::CreateTopics(r)).await else {
            panic!()
        };
        assert_eq!(r.topics[0].error_code, 0, "{name}");
    }

    pub async fn produce(
        &self,
        topic: &str,
        partition: i32,
        batch: Bytes,
        txn: Option<&str>,
    ) -> i64 {
        let mut p = PartitionProduceData::default();
        p.index = partition;
        p.records = Some(batch);
        let mut t = TopicProduceData::default();
        t.name = TopicName(StrBytes::from_string(topic.to_owned()));
        t.partition_data = vec![p];
        let mut r = ProduceRequest::default();
        r.transactional_id = txn.map(|id| TransactionalId(StrBytes::from_string(id.to_owned())));
        r.acks = -1;
        r.timeout_ms = 5000;
        r.topic_data = vec![t];
        let Response::Produce(r) = self.ask(9, Request::Produce(r)).await else {
            panic!()
        };
        let p = &r.responses[0].partition_responses[0];
        assert_eq!(p.error_code, 0, "produce to {topic}-{partition}: {p:?}");
        p.base_offset
    }

    pub async fn stop(mut self) {
        if let Some(s) = self.server.take() {
            s.shutdown().await;
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn rec(i: i64, key: Option<&str>, value: &[u8], headers: &[(&str, &str)]) -> Rec {
    Rec {
        timestamp_delta: i,
        offset_delta: None,
        key: key.map(|k| k.as_bytes().to_vec()),
        value: Some(value.to_vec()),
        headers: headers
            .iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect(),
    }
}

pub fn batch(
    recs: &[Rec],
    attributes: u16,
    body: Option<Vec<u8>>,
    producer: Option<(i64, i16)>,
) -> Bytes {
    let mut b = BatchBuilder::from_records(now_ms() - 10_000, recs)
        .last_offset_delta(i32::try_from(recs.len()).unwrap() - 1)
        .attributes(attributes);
    if let Some(body) = body {
        b = b.body(body);
    }
    if let Some((id, epoch)) = producer {
        b = b.producer(id, epoch, 0);
    }
    b.build()
}

pub fn plain(recs: &[Rec]) -> Bytes {
    batch(recs, 0, None, None)
}

pub fn json(order: &str) -> Vec<u8> {
    format!(r#"{{"order_id":"{order}","customer":"c-1","items":[{{"sku":"FV-TOM-V","qty":2}}],"total":14.8}}"#).into_bytes()
}

/// An open transaction `orders-eos` that wrote to partitions 0 and 1 of `orders`.
pub async fn open_transaction(w: &World) -> (i64, i16) {
    let mut init = InitProducerIdRequest::default();
    init.transactional_id = Some(TransactionalId(StrBytes::from_static_str("orders-eos")));
    init.transaction_timeout_ms = 60_000;
    let Response::InitProducerId(p) = w.ask(4, Request::InitProducerId(init)).await else {
        panic!()
    };
    assert_eq!(p.error_code, 0);
    let mut add = AddPartitionsToTxnRequest::default();
    add.v3_and_below_transactional_id = TransactionalId(StrBytes::from_static_str("orders-eos"));
    add.v3_and_below_producer_id = ProducerId(p.producer_id.0);
    add.v3_and_below_producer_epoch = p.producer_epoch;
    let mut topic = AddPartitionsToTxnTopic::default();
    topic.name = TopicName(StrBytes::from_static_str("orders"));
    topic.partitions = vec![0, 1];
    add.v3_and_below_topics.push(topic);
    let Response::AddPartitionsToTxn(r) = w.ask(3, Request::AddPartitionsToTxn(add)).await else {
        panic!()
    };
    assert_eq!(
        r.results_by_topic_v3_and_below[0].results_by_partition[0].partition_error_code,
        0
    );
    (p.producer_id.0, p.producer_epoch)
}

pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

pub const BINARY: [u8; 12] = [
    0x0a, 0x09, 0xff, 0x00, 0xfe, 0x80, 0x2d, 0x38, 0x38, 0x34, 0x30, 0x39,
];

/// The seeded world: see the module comment.
pub async fn seeded(name: &str, tweak: impl FnOnce(&mut BrokerConfig)) -> World {
    let w = world(name, tweak).await;
    w.create("orders", 3, &[]).await;
    w.create("orders.dlq", 1, &[]).await;
    w.create("customer.profiles", 1, &[("cleanup.policy", "compact")])
        .await;
    w.create("gz.events", 1, &[]).await;
    let h = [
        ("content-type", "application/json"),
        (
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ),
    ];
    let orders: Vec<Rec> = (0..4)
        .map(|i| rec(i, Some(&format!("ord-{i}")), &json(&format!("ord-{i}")), &h))
        .collect();
    w.produce("orders", 0, plain(&orders), None).await;
    let more: Vec<Rec> = (4..7)
        .map(|i| rec(i, Some(&format!("ord-{i}")), &json(&format!("ord-{i}")), &h))
        .collect();
    w.produce("orders", 1, plain(&more), None).await;
    let bin = [
        rec(0, Some("ord-bin"), &BINARY, &[]),
        rec(1, Some("ord-8"), &json("ord-8"), &[]),
    ];
    w.produce("orders", 2, plain(&bin), None).await;
    let profiles = [
        rec(0, Some("c-1"), b"{\"name\":\"a\"}", &[]),
        rec(1, Some("c-2"), b"{\"name\":\"b\"}", &[]),
        rec(2, Some("c-1"), b"{\"name\":\"a2\"}", &[]),
    ];
    w.produce("customer.profiles", 0, plain(&profiles), None)
        .await;
    let zipped: Vec<Rec> = (0..3)
        .map(|i| rec(i, Some(&format!("g-{i}")), &json(&format!("g-{i}")), &[]))
        .collect();
    w.produce(
        "gz.events",
        0,
        batch(&zipped, 1, Some(gzip(&encode_records(&zipped))), None),
        None,
    )
    .await;
    let (pid, epoch) = open_transaction(&w).await;
    let t0 = [
        rec(0, Some("txn-a"), &json("txn-a"), &h),
        rec(1, Some("txn-b"), &json("txn-b"), &h),
    ];
    w.produce(
        "orders",
        0,
        batch(&t0, 0x10, None, Some((pid, epoch))),
        Some("orders-eos"),
    )
    .await;
    let t1 = [rec(0, Some("txn-c"), &json("txn-c"), &h)];
    w.produce(
        "orders",
        1,
        batch(&t1, 0x10, None, Some((pid, epoch))),
        Some("orders-eos"),
    )
    .await;
    w
}

pub fn live_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff-live")
}

/// A page, asserted to be a 200, and written for the live board-diff under the board's id.
pub async fn page(app: &Router, path: &str, id: Option<&str>) -> Doc {
    let (res, body) = send(app, get(path)).await;
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "{path}: {}",
        Doc(body.clone())
    );
    if let Some(id) = id {
        let dir = live_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.html")), &body).unwrap();
    }
    Doc(body)
}

/// A page: its HTML to look into, its text when a failure message prints it.
pub struct Doc(String);

impl std::ops::Deref for Doc {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Doc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let main = self.0.split("<main").nth(1).unwrap_or(&self.0);
        let mut out = String::new();
        let mut tag = false;
        for c in main.chars() {
            match c {
                '<' => {
                    tag = true;
                    out.push(' ');
                }
                '>' => tag = false,
                _ if !tag => out.push(c),
                _ => {}
            }
        }
        let words: Vec<&str> = out.split_whitespace().collect();
        f.write_str(&words.join(" ").chars().take(2500).collect::<String>())
    }
}

pub fn row_count(body: &str) -> usize {
    body.matches("<a class=\"lnk mono\" href=\"/topics/")
        .count()
}
