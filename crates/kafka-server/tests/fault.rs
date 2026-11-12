//! Fault rules on a real socket with a real broker: a rule delays an answer, makes one up, or closes the connection, and the broker's
//! data is as if the request had never come. The recorder's side of it (the lab tag) is the console's tests.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use client::{Client, metadata_for};
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_protocol::protocol::StrBytes;
use kafka_server::fault::{Effect, FaultTable, Matcher, Pattern, Share, Spec};
use kafka_server::{Answer, Call, Config, ConnId, Observed, Observer, Opened, Pending, Running, Server};
use kafka_storage::sim::SimFs;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    ApiKey, FetchRequest, FetchResponse, MetadataResponse, ProduceRequest, ProduceResponse,
    TopicName,
};

/// What an observer was told of each request: the API, whether the connection was closed, and the rule that touched it.
#[derive(Debug, Default)]
struct Told(Mutex<Vec<(ApiKey, bool, Option<String>)>>);

impl Observer for Told {
    fn opened(&self, _: &Opened) {}
    fn begin(&self, call: &Call<'_>) -> Pending {
        Pending::new(call.head.key)
    }
    fn end(&self, pending: Pending, seen: &Observed<'_>) {
        let api = pending.into_payload::<ApiKey>().unwrap();
        let closed = matches!(seen.answer, Answer::Closed(_));
        let rule = seen.fault.map(|n| n.name.to_string());
        self.0.lock().unwrap().push((api, closed, rule));
    }
    fn closed(&self, _: ConnId, _: &str) {}
}

async fn start() -> (Running, Arc<FaultTable>, Arc<Told>) {
    let table = FaultTable::new();
    let told = Arc::new(Told::default());
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        fault: Some(table.clone()),
        observer: Some(told.clone()),
        ..Config::default()
    };
    let broker = Broker::open(SimFs::new(), BrokerConfig::new("/data".into())).unwrap();
    (Server::bind(config, Arc::new(broker)).await.unwrap(), table, told)
}

fn rule(name: &str, apis: &[ApiKey], topic: Option<&str>, effect: Effect) -> Spec {
    Spec {
        name: name.to_owned(),
        matcher: Matcher {
            apis: apis.to_vec(),
            topics: topic.map(|t| Pattern::new(t).unwrap()).into_iter().collect(),
            ..Matcher::default()
        },
        effect,
        share: Share::Every,
        ttl: Duration::from_secs(120),
        max_hits: None,
    }
}

async fn make_topic(c: &mut Client, name: &str) {
    // The first Metadata for a topic that is not there makes it and says it is not there yet.
    for _ in 0..20 {
        let r: MetadataResponse = c
            .call(ApiKey::Metadata, 9, &metadata_for(name))
            .await
            .unwrap();
        if r.topics[0].error_code == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the topic {name} was not made");
}

fn produce_request(topic: &str, acks: i16) -> ProduceRequest {
    let mut p = PartitionProduceData::default();
    p.index = 0;
    p.records = Some(BatchBuilder::new().records(2).build());
    let mut t = TopicProduceData::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partition_data.push(p);
    let mut r = ProduceRequest::default();
    r.acks = acks;
    r.timeout_ms = 5000;
    r.topic_data.push(t);
    r
}

async fn fetch_end(c: &mut Client, topic: &str) -> i64 {
    let mut p = FetchPartition::default();
    p.partition = 0;
    p.partition_max_bytes = 1 << 20;
    let mut t = FetchTopic::default();
    t.topic = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partitions.push(p);
    let mut r = FetchRequest::default();
    r.max_wait_ms = 0;
    r.max_bytes = 1 << 20;
    r.topics.push(t);
    let a: FetchResponse = c.call(ApiKey::Fetch, 12, &r).await.unwrap();
    let part = &a.responses[0].partitions[0];
    assert_eq!(part.error_code, 0);
    part.high_watermark
}

#[tokio::test]
async fn a_slow_rule_delays_the_answer_by_its_delay_and_tags_the_request() {
    let (server, table, told) = start().await;
    let mut c = Client::connect(&server).await;
    make_topic(&mut c, "orders").await;
    let began = Instant::now();
    let _: ProduceResponse = c.call(ApiKey::Produce, 9, &produce_request("orders", 1)).await.unwrap();
    let plain = began.elapsed();
    table
        .add(rule(
            "Slow produce",
            &[ApiKey::Produce],
            Some("orders"),
            Effect::Slow { delay_ms: 300, jitter_ms: 0 },
        ))
        .unwrap();
    let began = Instant::now();
    let r: ProduceResponse = c.call(ApiKey::Produce, 9, &produce_request("orders", 1)).await.unwrap();
    let slow = began.elapsed();
    assert_eq!(r.responses[0].partition_responses[0].error_code, 0);
    assert!(slow >= Duration::from_millis(300), "{slow:?} (plain {plain:?})");
    assert!(slow < Duration::from_millis(2500), "{slow:?}");
    // Both records of both Produce requests are there: a delayed request is a request served, late.
    assert_eq!(fetch_end(&mut c, "orders").await, 4);
    let seen = told.0.lock().unwrap().clone();
    let produces: Vec<_> = seen.iter().filter(|s| s.0 == ApiKey::Produce).collect();
    assert_eq!(produces[0].2, None);
    assert_eq!(produces[1].2.as_deref(), Some("Slow produce"));
    server.shutdown().await;
}

#[tokio::test]
async fn a_fail_rule_answers_with_its_code_and_the_broker_never_sees_the_request() {
    let (server, table, told) = start().await;
    let mut c = Client::connect(&server).await;
    make_topic(&mut c, "orders").await;
    let id = table
        .add(rule(
            "Fail produce",
            &[ApiKey::Produce],
            Some("orders"),
            Effect::Fail { code: 7 },
        ))
        .unwrap();
    let r: ProduceResponse = c.call(ApiKey::Produce, 9, &produce_request("orders", 1)).await.unwrap();
    let part = &r.responses[0].partition_responses[0];
    assert_eq!((part.index, part.error_code, part.base_offset), (0, 7, -1));
    assert_eq!(fetch_end(&mut c, "orders").await, 0, "nothing was appended");
    // A produce that asks for no acknowledgement gets none, and appends nothing either.
    c.send(ApiKey::Produce, 9, &produce_request("orders", 0)).await;
    assert_eq!(fetch_end(&mut c, "orders").await, 0);
    assert_eq!(table.rule(id).unwrap().hits, 2);
    // Another topic is served as usual.
    make_topic(&mut c, "payments").await;
    let r: ProduceResponse = c.call(ApiKey::Produce, 9, &produce_request("payments", 1)).await.unwrap();
    assert_eq!(r.responses[0].partition_responses[0].error_code, 0);
    let seen = told.0.lock().unwrap().clone();
    assert!(seen.iter().any(|s| s.0 == ApiKey::Produce && s.2.as_deref() == Some("Fail produce") && !s.1));
    server.shutdown().await;
}

#[tokio::test]
async fn a_drop_rule_closes_the_connection_and_the_next_one_is_served_when_the_rule_is_gone() {
    let (server, table, told) = start().await;
    let mut c = Client::connect(&server).await;
    make_topic(&mut c, "orders").await;
    let id = table
        .add(rule("Drop metadata", &[ApiKey::Metadata], None, Effect::Drop))
        .unwrap();
    let a: Option<MetadataResponse> = c.call(ApiKey::Metadata, 9, &metadata_for("orders")).await;
    assert!(a.is_none(), "the connection was closed with no answer");
    assert!(table.remove(id));
    let mut again = Client::connect(&server).await;
    make_topic(&mut again, "orders").await;
    let seen = told.0.lock().unwrap().clone();
    assert!(seen.iter().any(|s| s.0 == ApiKey::Metadata && s.1 && s.2.as_deref() == Some("Drop metadata")));
    server.shutdown().await;
}

#[tokio::test]
async fn a_drop_rule_by_client_closes_a_new_connection_at_its_first_request() {
    let (server, table, _) = start().await;
    let mut spec = rule("Keep dropping t", &[], None, Effect::Drop);
    spec.matcher.client = Some(Pattern::new("t").unwrap());
    table.add(spec).unwrap();
    let mut c = Client::connect(&server).await;
    assert!(c.versions().await.is_none(), "closed at ApiVersions");
    server.shutdown().await;
}

#[tokio::test]
async fn a_rule_that_matches_everything_does_not_touch_the_login() {
    let (server, table, _) = start_sasl().await;
    table
        .add(rule("Everything", &[], None, Effect::Drop))
        .unwrap();
    let mut c = Client::connect(&server).await;
    c.log_in(b"\0alice\0alice-secret-planted-7f3a").await;
    // The rule is in force all the same: the first request after the login is the one it closes.
    let a: Option<MetadataResponse> = c.call(ApiKey::Metadata, 9, &metadata_for("orders")).await;
    assert!(a.is_none());
    server.shutdown().await;
}

async fn start_sasl() -> (Running, Arc<FaultTable>, Arc<Told>) {
    use kafka_auth::{Authenticator, Credentials, Mechanism};
    let table = FaultTable::new();
    let told = Arc::new(Told::default());
    let users = [("alice", "alice-secret-planted-7f3a")].map(|(u, p)| (u.to_owned(), p.to_owned()));
    let auth = Authenticator::new(&[Mechanism::Plain], Credentials::new(users).unwrap()).unwrap();
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        sasl: Some(Arc::new(auth)),
        fault: Some(table.clone()),
        observer: Some(told.clone()),
        ..Config::default()
    };
    let broker = Broker::open(SimFs::new(), BrokerConfig::new("/data".into())).unwrap();
    (Server::bind(config, Arc::new(broker)).await.unwrap(), table, told)
}

#[tokio::test]
async fn stopping_the_server_does_not_wait_for_a_delayed_answer() {
    let (server, table, _) = start().await;
    let mut c = Client::connect(&server).await;
    make_topic(&mut c, "orders").await;
    table
        .add(rule(
            "Very slow",
            &[ApiKey::Metadata],
            None,
            Effect::Slow { delay_ms: 60_000, jitter_ms: 0 },
        ))
        .unwrap();
    c.send(ApiKey::Metadata, 9, &metadata_for("orders")).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let began = Instant::now();
    server.shutdown().await;
    assert!(began.elapsed() < Duration::from_secs(10), "{:?}", began.elapsed());
}
