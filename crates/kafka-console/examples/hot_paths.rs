//! Hot paths other than produce, measured on one in-memory broker called in-process (no sockets: on a shared machine their
//! noise hides a few microseconds), each request decoded from its bytes and its answer encoded as the server would
//! (produce over a socket is `produce_bench`):
//!
//! - `fetch`: a long-polling consumer's round on a partition that holds many aborted transactions, both isolation levels;
//! - `encode`: writing one large Fetch response frame;
//! - `groups`: classic members heartbeating their own groups, alone and while another client deletes groups.
//!
//! Prints the median and best of several rounds. Run: `cargo run --release -p kafka-console --example hot_paths [which]`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::panic
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes};
use kafka_broker::{Broker, Caller, Config as BrokerConfig, Listener, Outcome};
use kafka_protocol::messages::ResponseHeader;
use kafka_protocol::protocol::{Decodable, Encodable};
use kafka_storage::sim::SimFs;
use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::join_group_request::JoinGroupRequestProtocol;
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::*;

const ROUNDS: usize = 7;

type Running = Arc<Broker<SimFs>>;

struct Conn {
    broker: Running,
    listener: Listener,
    caller: Caller,
    id: i32,
}

impl Conn {
    #[allow(clippy::unused_async)]
    async fn open(broker: &Running) -> Self {
        Self {
            broker: Arc::clone(broker),
            listener: Listener {
                advertised_host: "127.0.0.1".into(),
                advertised_port: 9092,
                client_host: "/127.0.0.1".into(),
            },
            caller: Caller::anonymous(Some("127.0.0.1".parse().unwrap())),
            id: 0,
        }
    }

    async fn call<R: Encodable, A: Decodable>(&mut self, key: ApiKey, version: i16, body: &R) -> A {
        self.id += 1;
        let mut payload = bytes::BytesMut::new();
        body.encode(&mut payload, version).unwrap();
        let mut payload = payload.freeze();
        let request = Request::decode(key, version, &mut payload).unwrap();
        let head = RequestHead {
            key,
            version,
            correlation_id: self.id,
            client_id: None,
        };
        let Outcome::Respond(response) = self
            .broker
            .handle(&self.listener, &self.caller, &head, request)
            .await
        else {
            panic!("{key:?} not answered")
        };
        let frame = kafka_wire::response::encode(self.id, version, &response).unwrap();
        let mut b = frame.slice(4..);
        ResponseHeader::decode(&mut b, key.response_header_version(version)).unwrap();
        let answer = A::decode(&mut b, version).unwrap();
        assert!(!b.has_remaining(), "{key:?} read whole");
        answer
    }
}

fn name(s: &'static str) -> TopicName {
    TopicName(StrBytes::from_static_str(s))
}

#[allow(clippy::unused_async)]
async fn broker() -> Running {
    Arc::new(Broker::open(SimFs::new(), BrokerConfig::new("/data".into())).unwrap())
}

async fn create(c: &mut Conn, topic: &'static str) {
    let mut t = CreatableTopic::default();
    t.name = name(topic);
    t.num_partitions = 1;
    t.replication_factor = 1;
    let mut create = CreateTopicsRequest::default();
    create.topics = vec![t];
    let r: CreateTopicsResponse = c.call(ApiKey::CreateTopics, 7, &create).await;
    assert_eq!(r.topics[0].error_code, 0);
}

/// Calls per second of each round.
struct Measured {
    rates: Vec<f64>,
}

impl Measured {
    fn new() -> Self {
        Self { rates: Vec::new() }
    }

    fn add(&mut self, calls: f64, took: f64) {
        self.rates.push(calls / took);
    }
}

/// The median and the best: on a machine others are using too, the best is the steadier measure of the code.
fn summary(label: &str, unit: &str, m: &Measured) {
    let mut sorted = m.rates.clone();
    sorted.sort_by(f64::total_cmp);
    let (median, best) = (sorted[sorted.len() / 2], sorted[sorted.len() - 1]);
    println!(
        "{label:<44} median {median:>9.0} {unit}/s ({:>8.2} us each)   best {best:>9.0} {unit}/s ({:>8.2} us each)",
        1e6 / median,
        1e6 / best
    );
}

/// Rounds of `n` calls of `f`, the first a warm-up.
async fn rounds<F: AsyncFnMut()>(n: usize, mut f: F) -> Measured {
    let mut m = Measured::new();
    for round in 0..=ROUNDS {
        let began = Instant::now();
        for _ in 0..n {
            f().await;
        }
        if round > 0 {
            m.add(n as f64, began.elapsed().as_secs_f64());
        }
    }
    m
}

const ABORTED: i32 = 2_000;

/// A fetch of partition 0 of `txns` from `offset`, answered at once.
fn fetch_of(isolation: i8, offset: i64) -> FetchRequest {
    let mut p = FetchPartition::default();
    p.partition = 0;
    p.fetch_offset = offset;
    p.partition_max_bytes = 1 << 20;
    let mut t = FetchTopic::default();
    t.topic = name("txns");
    t.partitions = vec![p];
    let mut req = FetchRequest::default();
    req.replica_id = BrokerId(-1);
    req.max_wait_ms = 0;
    req.min_bytes = 1;
    req.max_bytes = 1 << 20;
    req.isolation_level = isolation;
    req.topics = vec![t];
    req
}

/// Topic `txns` with `ABORTED` one-record transactions, each aborted; the end of its log once every marker is written.
async fn aborted_log(c: &mut Conn) -> i64 {
    create(c, "txns").await;
    let id = || TransactionalId(StrBytes::from_static_str("bench-tx"));
    let mut init = InitProducerIdRequest::default();
    init.transactional_id = Some(id());
    init.transaction_timeout_ms = 60_000;
    let ids: InitProducerIdResponse = c.call(ApiKey::InitProducerId, 4, &init).await;
    assert_eq!(ids.error_code, 0);
    let (producer, epoch) = (ids.producer_id.0, ids.producer_epoch);
    for seq in 0..ABORTED {
        let mut add = AddPartitionsToTxnRequest::default();
        add.v3_and_below_transactional_id = id();
        add.v3_and_below_producer_id = ProducerId(producer);
        add.v3_and_below_producer_epoch = epoch;
        let mut t = AddPartitionsToTxnTopic::default();
        t.name = name("txns");
        t.partitions = vec![0];
        add.v3_and_below_topics.push(t);
        let _: AddPartitionsToTxnResponse = c.call(ApiKey::AddPartitionsToTxn, 3, &add).await;
        let mut p = PartitionProduceData::default();
        p.records = Some(
            BatchBuilder::new()
                .records(1)
                .attributes(0x10)
                .timestamps(1_000, 1_000)
                .producer(producer, epoch, seq)
                .build(),
        );
        let mut t = TopicProduceData::default();
        t.name = name("txns");
        t.partition_data = vec![p];
        let mut r = ProduceRequest::default();
        r.acks = -1;
        r.timeout_ms = 5_000;
        r.transactional_id = Some(id());
        r.topic_data = vec![t];
        let produced: ProduceResponse = c.call(ApiKey::Produce, 9, &r).await;
        assert_eq!(produced.responses[0].partition_responses[0].error_code, 0);
        let mut end = EndTxnRequest::default();
        end.transactional_id = id();
        end.producer_id = ProducerId(producer);
        end.producer_epoch = epoch;
        end.committed = false;
        let ended: EndTxnResponse = c.call(ApiKey::EndTxn, 3, &end).await;
        assert_eq!(ended.error_code, 0);
    }
    // Markers are written by the coordinator after EndTxn answers: wait until the log end is still.
    let mut end = 0;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let r: FetchResponse = c.call(ApiKey::Fetch, 12, &fetch_of(0, 0)).await;
        let hw = r.responses[0].partitions[0].high_watermark;
        if hw == end && hw >= i64::from(2 * ABORTED) {
            break;
        }
        end = hw;
    }
    end
}

async fn fetch() {
    let server = broker().await;
    let mut c = Conn::open(&server).await;
    let end = aborted_log(&mut c).await;
    println!(
        "{ABORTED} aborted transactions, log end {end}; a fetch at the end with max_wait 0 (one round, no data)"
    );
    let uncommitted = fetch_of(0, end);
    let rates = rounds(5_000, async || {
        let r: FetchResponse = c.call(ApiKey::Fetch, 12, &uncommitted).await;
        assert_eq!(r.responses[0].partitions[0].error_code, 0);
    })
    .await;
    summary("fetch read_uncommitted at the end", "req", &rates);
    let committed = fetch_of(1, end);
    let rates = rounds(5_000, async || {
        let r: FetchResponse = c.call(ApiKey::Fetch, 12, &committed).await;
        let p = &r.responses[0].partitions[0];
        assert_eq!(p.error_code, 0);
        assert_eq!(p.aborted_transactions.as_ref().map(Vec::len), Some(0));
    })
    .await;
    summary("fetch read_committed at the end", "req", &rates);
    let from_start = fetch_of(1, 0);
    let rates = rounds(1_000, async || {
        let r: FetchResponse = c.call(ApiKey::Fetch, 12, &from_start).await;
        let p = &r.responses[0].partitions[0];
        assert_eq!(p.error_code, 0);
        assert!(
            p.aborted_transactions
                .as_ref()
                .is_some_and(|a| !a.is_empty())
        );
    })
    .await;
    summary("fetch read_committed from 0 (1 MiB)", "req", &rates);
}

/// A Fetch response of `batches` batches of about 100 KiB each.
fn big_fetch(batches: usize) -> kafka_wire::api::Response {
    let rec = Rec {
        key: None,
        value: Some(vec![b'v'; 100 << 10]),
        ..Rec::default()
    };
    let one: Bytes = BatchBuilder::from_records(1_700_000_000_000, &[rec])
        .last_offset_delta(0)
        .build();
    let mut records = Vec::with_capacity(one.len() * batches);
    for _ in 0..batches {
        records.extend_from_slice(&one);
    }
    let mut p = fetch_response::PartitionData::default();
    p.records = Some(Bytes::from(records));
    let mut t = fetch_response::FetchableTopicResponse::default();
    t.topic = name("big");
    t.partitions = vec![p];
    let mut r = FetchResponse::default();
    r.responses = vec![t];
    kafka_wire::api::Response::Fetch(r)
}

/// Both ways the crate writes a Fetch: into one buffer (`encode`, the records copied) and as the frame of pieces the server
/// writes (`frame`, the records shared; the socket gathers them).
fn encode() {
    for batches in [1_usize, 10, 80] {
        let response = big_fetch(batches);
        for (how, shared) in [("encode", false), ("frame", true)] {
            let mut m = Measured::new();
            let n = 4_000 / batches;
            for round in 0..=ROUNDS {
                let began = Instant::now();
                for _ in 0..n {
                    if shared {
                        std::hint::black_box(
                            kafka_wire::response::frame(1, 12, &response).unwrap(),
                        );
                    } else {
                        std::hint::black_box(
                            kafka_wire::response::encode(1, 12, &response).unwrap(),
                        );
                    }
                }
                if round > 0 {
                    m.add(n as f64, began.elapsed().as_secs_f64());
                }
            }
            summary(
                &format!("{how} a Fetch response of {batches} x 100 KiB"),
                "resp",
                &m,
            );
        }
    }
}

#[path = "hot_paths/groups.rs"]
mod groups;
use groups::groups;

#[tokio::main]
async fn main() {
    let which = std::env::args().nth(1).unwrap_or_default();
    if which.is_empty() || which == "fetch" {
        fetch().await;
    }
    if which.is_empty() || which == "encode" {
        encode();
    }
    if which.is_empty() || which == "groups" {
        groups().await;
    }
    if which.is_empty() || which == "produce" {
        produce().await;
    }
}

/// One-record batches (acks 1) to a topic with the configs a produce reads set on it.
async fn produce() {
    let server = broker().await;
    let mut c = Conn::open(&server).await;
    let mut t = CreatableTopic::default();
    t.name = name("configured");
    t.num_partitions = 1;
    t.replication_factor = 1;
    for (k, v) in [
        ("compression.type", "producer"),
        ("max.message.bytes", "1048588"),
        ("message.timestamp.type", "CreateTime"),
        ("message.timestamp.before.max.ms", "9223372036854775807"),
        ("message.timestamp.after.max.ms", "3600000"),
        ("cleanup.policy", "delete"),
        ("retention.ms", "604800000"),
    ] {
        let mut config = create_topics_request::CreatableTopicConfig::default();
        config.name = StrBytes::from_static_str(k);
        config.value = Some(StrBytes::from_static_str(v));
        t.configs.push(config);
    }
    let mut create = CreateTopicsRequest::default();
    create.topics = vec![t];
    let r: CreateTopicsResponse = c.call(ApiKey::CreateTopics, 7, &create).await;
    assert_eq!(r.topics[0].error_code, 0);
    let rec = Rec {
        key: Some(b"k".to_vec()),
        value: Some(vec![b'v'; 100]),
        ..Rec::default()
    };
    let mut p = PartitionProduceData::default();
    p.records = Some(
        BatchBuilder::from_records(1_700_000_000_000, &[rec])
            .last_offset_delta(0)
            .build(),
    );
    let mut td = TopicProduceData::default();
    td.name = name("configured");
    td.partition_data = vec![p];
    let mut req = ProduceRequest::default();
    req.acks = 1;
    req.timeout_ms = 5_000;
    req.topic_data = vec![td];
    let m = rounds(20_000, async || {
        let r: ProduceResponse = c.call(ApiKey::Produce, 9, &req).await;
        assert_eq!(r.responses[0].partition_responses[0].error_code, 0);
    })
    .await;
    summary("produce, a configured topic", "req", &m);
}
