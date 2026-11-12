//! The Topics pages over a real broker in the same process, as the principal `User:__console`: seeded with topics of several
//! shapes (a compacted one, a gzip batch, headers, a value that is not UTF-8, an open transaction, a topic with a 249-byte
//! name), every route requested through `Router::oneshot`, and the answers compared with the boards on structure (the live
//! render of `tools/board-diff.ts`, written to `target/board-diff-live/`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::http::StatusCode;
use bytes::Bytes;
use kafka_auth::Principal;
use kafka_broker::{Broker, Caller, Config as BrokerConfig, Listener, Outcome};
use kafka_console::base::Base;
use kafka_console::facts::Facts;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_console::Backend;
use kafka_server::{Config as ServerConfig, Running, Server};
use kafka_storage::fs::RealFs;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::build::{BatchBuilder, Rec, encode_records};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::add_partitions_to_txn_request::AddPartitionsToTxnTopic;
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::create_topics_request::{CreatableTopic, CreatableTopicConfig};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    AddPartitionsToTxnRequest, CreateAclsRequest, CreateTopicsRequest, InitProducerIdRequest,
    ProduceRequest, ProducerId, TopicName, TransactionalId,
};
use support::{get, send};
use tokio_util::sync::CancellationToken;

struct World {
    dir: PathBuf,
    broker: Arc<Broker<RealFs>>,
    server: Option<Running>,
    backend: Backend,
    facts: Facts,
}

async fn world(name: &str, tweak: impl FnOnce(&mut BrokerConfig)) -> World {
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
    World { dir, broker, server: Some(server), backend, facts }
}

fn now_ms() -> i64 {
    kafka_console::source::now_ms()
}

fn router(backend: Backend, facts: Facts) -> Router {
    let state = State::new(
        Source::Live(Box::new(Live::new(backend, facts))),
        Base::root(),
        vec![],
        CancellationToken::new(),
    );
    kafka_console::app::router(state)
}

impl World {
    fn router(&self) -> Router {
        router(self.backend.clone(), self.facts.clone())
    }

    fn router_as(&self, caller: Caller) -> Router {
        router(self.backend.clone().acting_as(caller), self.facts.clone())
    }

    async fn ask(&self, version: i16, request: Request) -> Response {
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

    async fn create(&self, name: &str, partitions: i32, configs: &[(&str, &str)]) {
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
        let Response::CreateTopics(r) = self.ask(7, Request::CreateTopics(r)).await else { panic!() };
        assert_eq!(r.topics[0].error_code, 0, "{name}");
    }

    async fn produce(&self, topic: &str, partition: i32, batch: Bytes, txn: Option<&str>) -> i64 {
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
        let Response::Produce(r) = self.ask(9, Request::Produce(r)).await else { panic!() };
        let p = &r.responses[0].partition_responses[0];
        assert_eq!(p.error_code, 0, "produce to {topic}-{partition}: {p:?}");
        p.base_offset
    }

    async fn stop(mut self) {
        if let Some(s) = self.server.take() {
            s.shutdown().await;
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rec(i: i64, key: Option<&str>, value: &[u8], headers: &[(&str, &str)]) -> Rec {
    Rec {
        timestamp_delta: i,
        offset_delta: None,
        key: key.map(|k| k.as_bytes().to_vec()),
        value: Some(value.to_vec()),
        headers: headers.iter().map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec())).collect(),
    }
}

fn batch(recs: &[Rec], attributes: u16, body: Option<Vec<u8>>, producer: Option<(i64, i16)>) -> Bytes {
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

fn plain(recs: &[Rec]) -> Bytes {
    batch(recs, 0, None, None)
}

fn json(order: &str) -> Vec<u8> {
    format!(r#"{{"order_id":"{order}","customer":"c-1","items":[{{"sku":"FV-TOM-V","qty":2}}],"total":14.8}}"#).into_bytes()
}

/// An open transaction `orders-eos` that wrote to partitions 0 and 1 of `orders`.
async fn open_transaction(w: &World) -> (i64, i16) {
    let mut init = InitProducerIdRequest::default();
    init.transactional_id = Some(TransactionalId(StrBytes::from_static_str("orders-eos")));
    init.transaction_timeout_ms = 60_000;
    let Response::InitProducerId(p) = w.ask(4, Request::InitProducerId(init)).await else { panic!() };
    assert_eq!(p.error_code, 0);
    let mut add = AddPartitionsToTxnRequest::default();
    add.v3_and_below_transactional_id = TransactionalId(StrBytes::from_static_str("orders-eos"));
    add.v3_and_below_producer_id = ProducerId(p.producer_id.0);
    add.v3_and_below_producer_epoch = p.producer_epoch;
    let mut topic = AddPartitionsToTxnTopic::default();
    topic.name = TopicName(StrBytes::from_static_str("orders"));
    topic.partitions = vec![0, 1];
    add.v3_and_below_topics.push(topic);
    let Response::AddPartitionsToTxn(r) = w.ask(3, Request::AddPartitionsToTxn(add)).await else { panic!() };
    assert_eq!(r.results_by_topic_v3_and_below[0].results_by_partition[0].partition_error_code, 0);
    (p.producer_id.0, p.producer_epoch)
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

const BINARY: [u8; 12] = [0x0a, 0x09, 0xff, 0x00, 0xfe, 0x80, 0x2d, 0x38, 0x38, 0x34, 0x30, 0x39];

/// The seeded world: see the module comment.
async fn seeded(name: &str, tweak: impl FnOnce(&mut BrokerConfig)) -> World {
    let w = world(name, tweak).await;
    w.create("orders", 3, &[]).await;
    w.create("orders.dlq", 1, &[]).await;
    w.create("customer.profiles", 1, &[("cleanup.policy", "compact")]).await;
    w.create("gz.events", 1, &[]).await;
    let h = [("content-type", "application/json"), ("traceparent", "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")];
    let orders: Vec<Rec> = (0..4).map(|i| rec(i, Some(&format!("ord-{i}")), &json(&format!("ord-{i}")), &h)).collect();
    w.produce("orders", 0, plain(&orders), None).await;
    let more: Vec<Rec> = (4..7).map(|i| rec(i, Some(&format!("ord-{i}")), &json(&format!("ord-{i}")), &h)).collect();
    w.produce("orders", 1, plain(&more), None).await;
    let bin = [rec(0, Some("ord-bin"), &BINARY, &[]), rec(1, Some("ord-8"), &json("ord-8"), &[])];
    w.produce("orders", 2, plain(&bin), None).await;
    let profiles = [rec(0, Some("c-1"), b"{\"name\":\"a\"}", &[]), rec(1, Some("c-2"), b"{\"name\":\"b\"}", &[]), rec(2, Some("c-1"), b"{\"name\":\"a2\"}", &[])];
    w.produce("customer.profiles", 0, plain(&profiles), None).await;
    let zipped: Vec<Rec> = (0..3).map(|i| rec(i, Some(&format!("g-{i}")), &json(&format!("g-{i}")), &[])).collect();
    w.produce("gz.events", 0, batch(&zipped, 1, Some(gzip(&encode_records(&zipped))), None), None).await;
    let (pid, epoch) = open_transaction(&w).await;
    let t0 = [rec(0, Some("txn-a"), &json("txn-a"), &h), rec(1, Some("txn-b"), &json("txn-b"), &h)];
    w.produce("orders", 0, batch(&t0, 0x10, None, Some((pid, epoch))), Some("orders-eos")).await;
    let t1 = [rec(0, Some("txn-c"), &json("txn-c"), &h)];
    w.produce("orders", 1, batch(&t1, 0x10, None, Some((pid, epoch))), Some("orders-eos")).await;
    w
}

fn live_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff-live")
}

/// A page, asserted to be a 200, and written for the live board-diff under the board's id.
async fn page(app: &Router, path: &str, id: Option<&str>) -> Doc {
    let (res, body) = send(app, get(path)).await;
    assert_eq!(res.status(), StatusCode::OK, "{path}: {}", Doc(body.clone()));
    if let Some(id) = id {
        let dir = live_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.html")), &body).unwrap();
    }
    Doc(body)
}

/// A page: its HTML to look into, its text when a failure message prints it.
struct Doc(String);

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

fn row_count(body: &str) -> usize {
    body.matches("<td class=\"mono\"><a class=\"lnk\" href=\"/topics/").count()
}

#[tokio::test]
async fn every_route_answers_as_the_console_and_has_the_boards_parts() {
    let w = seeded("routes", |_| {}).await;
    let app = w.router();
    let list = page(&app, "/topics", Some("topics-list")).await;
    for name in ["orders", "orders.dlq", "customer.profiles", "gz.events"] {
        assert!(list.contains(&format!(">{name}</a>")), "{name} in {list:.2000}");
    }
    assert!(list.contains("4 topics hold <b>6</b> partitions."), "{list:.1500}");
    assert!(list.contains("Transaction open") && list.contains("empty: no records yet"), "{list:.3000}");
    assert!(list.contains("<span class=\"tag\">compact</span>"));
    page(&app, "/topics/orders?limit=5", Some("topic-detail")).await;
    let detail = page(&app, "/topics/orders", None).await;
    assert!(detail.contains("Transaction orders-eos is open."), "{detail:.3000}");
    assert!(detail.contains("Above LSO") && detail.contains("Committed"));
    assert!(detail.contains("Producers<span class=\"n\">1</span>"), "one active producer");
    assert!(detail.contains("binary · 0a 09 ff 00 fe 80 2d 38 …"), "the binary value");
    page(&app, "/topics/orders.dlq", Some("topic-messages-empty")).await;
    let lso = {
        let p = page(&app, "/topics/orders/partitions", Some("topic-partitions")).await;
        assert!(p.contains("held 2") && p.contains("held 1") && p.contains("clear"), "{p:.4000}");
        p
    };
    assert!(lso.contains("orders-0") && lso.contains("Delete records"));
    page(&app, "/topics/orders/messages?partition=0&offset=4", Some("topic-messages-filtered")).await;
    let rc = page(&app, "/topics/orders/messages?isolation=read_committed", Some("topic-readcommitted")).await;
    assert!(rc.contains("held back from read_committed consumers"), "{rc:.3000}");
    assert!(rc.contains("orders-eos began at offset 4"), "{rc:.5000}");
    let filtered = page(&app, "/topics?q=zzz", Some("topics-filtered")).await;
    assert!(filtered.contains("No topic name contains"));
    // A record of the open transaction and one of the binary value.
    let rec = page(&app, "/topics/orders/messages/0/4", Some("topic-record")).await;
    assert!(rec.contains("At or above the last stable offset."), "{rec:.3000}");
    assert!(rec.contains("orders-eos") && rec.contains("Ongoing") && rec.contains("txn-a"), "{rec:.4000}");
    let bin = page(&app, "/topics/orders/messages/2/0", Some("topic-record-binary")).await;
    assert!(bin.contains("The value is not text.") && bin.contains("0000  0a 09 ff 00 fe 80 2d 38"), "{bin:.3000}");
    assert!(bin.contains("12 bytes · binary"));
    w.stop().await;
}

#[tokio::test]
async fn an_empty_broker_is_the_none_state() {
    let w = world("none", |_| {}).await;
    let app = w.router();
    let body = page(&app, "/topics", Some("topics-none")).await;
    assert!(body.contains("This broker holds no topics"), "{body:.2000}");
    w.stop().await;
}

#[tokio::test]
async fn read_committed_stops_at_the_last_stable_offset_and_read_uncommitted_does_not() {
    let w = seeded("isolation", |_| {}).await;
    let app = w.router();
    let all = page(&app, "/topics/orders/messages?partition=0", None).await;
    let committed = page(&app, "/topics/orders/messages?partition=0&isolation=read_committed", None).await;
    assert_eq!(row_count(&all), 6, "four records and the two of the open transaction");
    assert_eq!(row_count(&committed), 4);
    assert_eq!(all.matches("st warn\">Above LSO").count(), 2);
    assert_eq!(committed.matches("st warn\">Above LSO").count(), 0);
    assert!(all.contains("txn-a") && !committed.contains("txn-a"));
    assert!(all.contains("2 of them are at or above the last stable offset") || all.contains("Newest first"), "{all:.2500}");
    w.stop().await;
}

#[tokio::test]
async fn a_compacted_topic_a_gzip_batch_headers_and_a_binary_value_are_shown_as_they_are() {
    let w = seeded("shapes", |_| {}).await;
    let app = w.router();
    let c = page(&app, "/topics/customer.profiles", None).await;
    assert!(c.contains("keeps the latest record of each key") && c.contains("compact"), "{c:.2500}");
    assert_eq!(row_count(&c), 3);
    let g = page(&app, "/topics/gz.events", None).await;
    assert_eq!(row_count(&g), 3);
    let one = page(&app, "/topics/gz.events/messages/0/1", None).await;
    assert!(one.contains("g-1") && one.contains("<dt>Codec</dt><dd>gzip</dd>"), "{one:.3000}");
    let h = page(&app, "/topics/orders/messages/0/0", None).await;
    assert!(h.contains("content-type") && h.contains("application/json") && h.contains("traceparent"), "{h:.3000}");
    assert!(h.contains("<dt>Codec</dt><dd>none</dd>") && h.contains("Position"));
    w.stop().await;
}

#[tokio::test]
async fn the_filters_find_by_key_by_offset_and_by_time_and_a_bad_value_is_a_sentence() {
    let w = seeded("filters", |_| {}).await;
    let app = w.router();
    let by_key = page(&app, "/topics/orders/messages?key=ord-5", None).await;
    assert_eq!(row_count(&by_key), 1, "{by_key:.3000}");
    assert!(by_key.contains("The search found"), "{by_key:.2500}");
    let none = page(&app, "/topics/orders/messages?key=nobody", None).await;
    assert!(none.contains("No record matched. The search looked at all"), "{none:.2500}");
    let from = page(&app, "/topics/orders/messages?partition=1&offset=1", None).await;
    assert_eq!(row_count(&from), 2 + 1, "ord-5, ord-6 and the open transaction's record, from offset 1");
    let ms = now_ms() - 20_000;
    let by_time = page(&app, &format!("/topics/orders/messages?partition=0&time={ms}"), None).await;
    assert!(row_count(&by_time) >= 1, "{by_time:.2500}");
    let bad = page(&app, "/topics/orders/messages?partition=99&offset=-1&time=yesterday&limit=abc&isolation=x", None).await;
    for sentence in ["is not an offset", "is not a time", "is not a number of records", "is not an isolation level", "has no partition 99"] {
        assert!(bad.contains(sentence), "{sentence}: {bad:.3000}");
    }
    let huge = page(&app, "/topics/orders/messages?limit=99999999999999999999", None).await;
    assert!(huge.contains("is not a number of records"));
    let capped = page(&app, "/topics/orders/messages?limit=100000", None).await;
    assert!(capped.contains("shows at most 200 records"));
    w.stop().await;
}

#[tokio::test]
async fn a_topic_name_of_249_bytes_fits_and_a_longer_one_is_no_topic() {
    let w = world("long", |_| {}).await;
    let name = "n".repeat(249);
    w.create(&name, 1, &[]).await;
    let app = w.router();
    let list = page(&app, "/topics", None).await;
    assert!(list.contains(&format!(">{name}</a>")));
    let t = page(&app, &format!("/topics/{name}"), None).await;
    assert!(t.contains(&format!("<h1 class=\"mono\">{name}</h1>")));
    let (res, _) = send(&app, get(&format!("/topics/{}", "n".repeat(250)))).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let (res, _) = send(&app, get("/topics/no-such-topic")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let (res, _) = send(&app, get("/topics/orders/messages/0/99999999999999999999")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    w.stop().await;
}

#[tokio::test]
async fn a_record_past_the_end_is_a_page_that_says_so_and_not_an_error() {
    let w = seeded("past", |_| {}).await;
    let app = w.router();
    for path in ["/topics/orders/messages/0/9999", "/topics/orders/messages/9/0", "/topics/orders/messages/-1/0"] {
        let (res, body) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}");
        assert!(body.contains("<html"), "{path} is a page in the shell");
    }
    w.stop().await;
}

fn creation(topic: &str, principal: &str, op: i8) -> AclCreation {
    let mut c = AclCreation::default();
    c.resource_type = 2;
    c.resource_name = StrBytes::from_string(topic.to_owned());
    c.resource_pattern_type = 3;
    c.principal = StrBytes::from_string(principal.to_owned());
    c.host = StrBytes::from_static_str("*");
    c.operation = op;
    c.permission_type = 3;
    c
}

#[tokio::test]
async fn on_an_acl_broker_a_principal_that_may_describe_but_not_read_is_refused_in_kafkas_words() {
    let w = seeded("acl", |c| c.acl_default_allow = false).await;
    let mut r = CreateAclsRequest::default();
    r.creations = vec![creation("orders", "User:viewer", 8)];
    let Response::CreateAcls(r) = w.ask(3, Request::CreateAcls(r)).await else { panic!() };
    assert_eq!(r.results[0].error_code, 0);
    let viewer = Caller::new(Principal::user("viewer"), Some("127.0.0.1".parse().unwrap()));
    let app = w.router_as(viewer);
    let list = page(&app, "/topics", None).await;
    assert!(list.contains(">orders</a>") && !list.contains(">gz.events</a>"), "only what it may describe: {list:.2500}");
    let t = page(&app, "/topics/orders", None).await;
    assert!(t.contains("The broker refused the read: TOPIC_AUTHORIZATION_FAILED (29)."), "{t:.3000}");
    assert_eq!(row_count(&t), 0);
    let (res, _) = send(&app, get("/topics/gz.events")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "a topic it may not describe is not there");
    // Somebody with no right at all sees an empty broker.
    let nobody = Caller::new(Principal::user("nobody"), Some("127.0.0.1".parse().unwrap()));
    let none = page(&w.router_as(nobody), "/topics", None).await;
    assert!(none.contains("This broker holds no topics"), "{none:.1500}");
    // The console itself, a super user, reads everything.
    let all = page(&w.router(), "/topics/orders", None).await;
    assert!(all.contains("ord-5"));
    w.stop().await;
}
