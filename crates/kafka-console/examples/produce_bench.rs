//! The hot-path claim, measured: one connection producing one-record batches (acks 1) to an in-memory broker, with no observer
//! installed (what `--console=false` runs), with the console's recorder installed, and with the recorder and a page open
//! (a stream viewer, so every request also bumps the tick). Prints requests per second and the cost per request of each, as
//! the median and the best of several rounds. Run: `cargo run --release -p kafka-console --example produce_bench`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::print_stdout
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_console::Recorder;
use kafka_protocol::protocol::Encodable;
use kafka_server::{Config, Observer, Server};
use kafka_storage::sim::SimFs;
use kafka_wire::StrBytes;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::*;
use kafka_wire::testing::{framed, request_payload};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const ROUNDS: usize = 7;
const REQUESTS: usize = 20_000;

async fn call<R: Encodable>(s: &mut TcpStream, key: ApiKey, version: i16, id: i32, body: &R) {
    let frame = framed(&request_payload(key, version, id, Some("bench"), body));
    s.write_all(&frame).await.unwrap();
    let mut len = [0; 4];
    s.read_exact(&mut len).await.unwrap();
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    s.read_exact(&mut payload).await.unwrap();
}

fn produce() -> ProduceRequest {
    let rec = Rec {
        key: Some(b"k".to_vec()),
        value: Some(vec![b'v'; 100]),
        ..Rec::default()
    };
    let batch: Bytes = BatchBuilder::from_records(1_700_000_000_000, &[rec])
        .last_offset_delta(0)
        .build();
    let mut p = PartitionProduceData::default();
    p.records = Some(batch);
    let mut t = TopicProduceData::default();
    t.name = TopicName(StrBytes::from_static_str("bench"));
    t.partition_data = vec![p];
    let mut r = ProduceRequest::default();
    r.acks = 1;
    r.topic_data = vec![t];
    r
}

/// One configuration: the requests per second of each round.
async fn measure(observer: Option<Arc<dyn Observer>>, listen: bool) -> Vec<f64> {
    let broker = Arc::new(Broker::open(SimFs::new(), BrokerConfig::new("/data".into())).unwrap());
    let config = Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        observer,
        ..Config::default()
    };
    let server = Server::bind(config, Arc::clone(&broker)).await.unwrap();
    let _ = listen;
    let mut s = TcpStream::connect(server.local_addr()).await.unwrap();
    s.set_nodelay(true).unwrap();
    let mut topic = CreatableTopic::default();
    topic.name = TopicName(StrBytes::from_static_str("bench"));
    topic.num_partitions = 1;
    topic.replication_factor = 1;
    let mut create = CreateTopicsRequest::default();
    create.topics = vec![topic];
    call(&mut s, ApiKey::CreateTopics, 7, 1, &create).await;
    let request = produce();
    let mut rates = Vec::new();
    for round in 0..ROUNDS + 1 {
        let began = Instant::now();
        for i in 0..REQUESTS {
            call(
                &mut s,
                ApiKey::Produce,
                9,
                i32::try_from(i).unwrap() + 2,
                &request,
            )
            .await;
        }
        // The first round warms the connection and the allocator.
        if round > 0 {
            rates.push(REQUESTS as f64 / began.elapsed().as_secs_f64());
        }
    }
    server.shutdown().await;
    rates
}

fn summary(label: &str, rates: &[f64]) -> (f64, f64) {
    let mut sorted = rates.to_vec();
    sorted.sort_by(f64::total_cmp);
    let (median, best) = (sorted[sorted.len() / 2], sorted[sorted.len() - 1]);
    println!(
        "{label:<34} median {median:>9.0} req/s ({:>6.1} us each)   best {best:>9.0} req/s ({:>6.1} us each)",
        1e6 / median,
        1e6 / best
    );
    (median, best)
}

#[tokio::main]
async fn main() {
    println!(
        "{REQUESTS} produce requests of one 100-byte record per round, {ROUNDS} rounds, one connection, in-memory broker"
    );
    let none = measure(None, false).await;
    let (m0, b0) = summary("no observer (console off)", &none);
    let recorder: Arc<dyn Observer> = Recorder::new();
    let with = measure(Some(recorder), false).await;
    let (m1, b1) = summary("recorder installed", &with);
    // A console page is open: every recorded request bumps the shared tick (at most every 100 ms).
    let rec = Recorder::new();
    let ticker = kafka_console::tick::Ticker::new(Duration::from_secs(3600));
    rec.attach(ticker.clone());
    let _viewer = ticker.watch();
    let watched = measure(Some(rec as Arc<dyn Observer>), true).await;
    let (m2, b2) = summary("recorder installed, a page open", &watched);
    let none_again = measure(None, false).await;
    let (m3, b3) = summary("no observer again (the noise)", &none_again);
    println!(
        "recorder vs none: median {:+.1}%, best {:+.1}%; page open vs none: median {:+.1}%, best {:+.1}%; none vs none again: median {:+.1}%, best {:+.1}%",
        (m1 / m0 - 1.0) * 100.0,
        (b1 / b0 - 1.0) * 100.0,
        (m2 / m0 - 1.0) * 100.0,
        (b2 / b0 - 1.0) * 100.0,
        (m3 / m0 - 1.0) * 100.0,
        (b3 / b0 - 1.0) * 100.0
    );
}
