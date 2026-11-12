//! The recorder and Live over a real broker: real client traffic over the Kafka listener (a raw client that speaks the wire), the
//! tape that results, its drawers, the clients, the docks and the first-request page, as the console principal. The live render
//! of the boards is written to `target/board-diff-live/`.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_write;

use std::sync::Arc;

use axum::Router;
use axum::http::StatusCode;
use bytes::Bytes;
use kafka_console::base::Base;
use kafka_console::model::WireQuery;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_console::Recorder;
use kafka_protocol::protocol::Encodable;
use kafka_wire::StrBytes;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::*;
use kafka_wire::testing::{framed, request_payload};
use support::get;
use support_write::{World, page, world};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

/// A client that speaks the Kafka wire over a real connection.
struct Wire {
    stream: TcpStream,
    next: i32,
}

impl Wire {
    async fn connect(w: &World) -> Self {
        let addr = w.server.as_ref().unwrap().local_addr();
        Self { stream: TcpStream::connect(addr).await.unwrap(), next: 1 }
    }

    /// One request and its whole answer (the payload after the length).
    async fn call<R: Encodable>(&mut self, key: ApiKey, version: i16, client: &str, body: &R) -> Bytes {
        let id = self.next;
        self.next += 1;
        self.stream.write_all(&framed(&request_payload(key, version, id, Some(client), body))).await.unwrap();
        let mut len = [0; 4];
        self.stream.read_exact(&mut len).await.unwrap();
        let mut payload = vec![0; u32::from_be_bytes(len) as usize];
        self.stream.read_exact(&mut payload).await.unwrap();
        Bytes::from(payload)
    }
}

fn s(text: &str) -> StrBytes {
    StrBytes::from_string(text.to_owned())
}

fn versions(software: &str) -> ApiVersionsRequest {
    let mut r = ApiVersionsRequest::default();
    r.client_software_name = s(software);
    r.client_software_version = s("2.3.0");
    r
}

fn produce(topic: &str, batch: Bytes, txn: Option<&str>) -> ProduceRequest {
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

fn batch(key: Option<&str>, value: Vec<u8>, producer: Option<(i64, i16)>) -> Bytes {
    let rec = Rec { key: key.map(|k| k.as_bytes().to_vec()), value: Some(value), ..Rec::default() };
    let mut b = BatchBuilder::from_records(1_700_000_000_000, &[rec]).last_offset_delta(0);
    if let Some((id, epoch)) = producer {
        b = b.producer(id, epoch, 0).attributes(0x10);
    }
    b.build()
}

fn fetch(topic: &str) -> FetchRequest {
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

fn init(txn: &str) -> InitProducerIdRequest {
    let mut r = InitProducerIdRequest::default();
    r.transactional_id = Some(TransactionalId(s(txn)));
    r.transaction_timeout_ms = 60_000;
    r
}

fn router(w: &World, recorder: &Arc<Recorder>) -> Router {
    let live = Live::new(w.backend.clone(), w.facts.clone()).with_recorder(Arc::clone(recorder));
    let state = State::new(Source::Live(Box::new(live)), Base::root(), vec![], CancellationToken::new());
    kafka_console::app::router(state)
}

/// A world whose server has the recorder installed.
async fn recorded(name: &str, deny: bool) -> (World, Arc<Recorder>) {
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
async fn traffic(w: &World) {
    w.create_topic("orders", 1, &[]).await;
    w.create_topic("inventory.levels", 1, &[("cleanup.policy", "compact")]).await;
    let mut py = Wire::connect(w).await;
    py.call(ApiKey::ApiVersions, 3, "fulfilment", &versions("confluent-kafka-python")).await;
    py.call(ApiKey::Metadata, 12, "fulfilment", &MetadataRequest::default()).await;
    for _ in 0..25 {
        py.call(ApiKey::Fetch, 12, "fulfilment", &fetch("orders")).await;
    }
    let mut go = Wire::connect(w).await;
    go.call(ApiKey::ApiVersions, 3, "orders", &versions("sarama")).await;
    let first = go.call(ApiKey::InitProducerId, 4, "orders", &init("orders-eos")).await;
    go.call(ApiKey::InitProducerId, 4, "orders", &init("orders-eos")).await;
    // The first epoch (0) is fenced now: a produce that carries it is refused. The answer is: correlation id, a tagged-fields
    // byte (header v1), throttle (4), error (2), then the producer id (8) and epoch (2).
    let pid = i64::from_be_bytes(first[11..19].try_into().unwrap());
    go.call(ApiKey::Produce, 9, "orders", &produce("orders", batch(Some("k"), b"x".to_vec(), Some((pid, 0))), Some("orders-eos"))).await;
    let mut cli = Wire::connect(w).await;
    cli.call(ApiKey::ApiVersions, 3, "admin-cli", &versions("librdkafka")).await;
    cli.call(ApiKey::Produce, 9, "admin-cli", &produce("inventory.levels", batch(None, b"FV-TOM-V:12".to_vec(), None), None)).await;
    cli.call(ApiKey::Produce, 9, "admin-cli", &produce("orders", batch(Some("k"), vec![b'z'; 1_300_000], None), None)).await;
    cli.call(ApiKey::Produce, 9, "admin-cli", &produce("orders", batch(Some("k"), b"hello".to_vec(), None), None)).await;
    let mut bad = Wire::connect(w).await;
    bad.call(ApiKey::ApiVersions, 3, "<script>alert(1)</script>", &versions("<img src=x onerror=alert(2)>")).await;
}

#[tokio::test]
async fn the_tape_shows_what_the_clients_did_and_what_was_refused() {
    let (w, rec) = recorded("tape", false).await;
    traffic(&w).await;
    let app = router(&w, &rec);
    let (status, live) = page(&app, get("/live?idle=0")).await;
    assert_eq!(status, StatusCode::OK);
    for want in [
        "INVALID_RECORD (87)", "MESSAGE_TOO_LARGE (10)", "Refused", "fulfilment", "Produce", "Metadata", "orders-0", "× 25", "1 record", "caught up, no records",
        "Refused among the rows kept:", "data-extra=\"live-sentence\"", "requests in the last minute.",
    ] {
        assert!(live.contains(want), "{want:?} in {live:.6000}");
    }
    assert!(live.contains("INVALID_PRODUCER_EPOCH (47)") || live.contains("PRODUCER_FENCED (90)"), "the fenced producer");
    // A hostile client name and software are text.
    assert!(!live.contains("<script>alert(1)") && !live.contains("<img src=x"), "{live:.4000}");
    assert!(live.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    // Idle polls are hidden by default and the fold is one row.
    let (_, quiet) = page(&app, get("/live")).await;
    assert!(!quiet.contains("× 25") && quiet.contains("idle polls hidden"));
    w.stop().await;
}

#[tokio::test]
async fn each_refusal_opens_its_own_drawer_with_kafkas_words_and_the_rule() {
    let (w, rec) = recorded("drawers", false).await;
    traffic(&w).await;
    let app = router(&w, &rec);
    let tape = rec.tape(&WireQuery { hide_idle: false, limit: 100, ..WireQuery::default() }, kafka_console::source::now_ms());
    let id_of = |code: i16| tape.rows.iter().find(|r| r.code == code).map(|r| r.id).unwrap_or_else(|| panic!("no row with code {code}"));
    let drawer = |id: u64| {
        let app = app.clone();
        async move {
            let (status, d) = page(&app, get(&format!("/live/{id}"))).await;
            assert_eq!(status, StatusCode::OK, "{d:.500}");
            d
        }
    };
    let refused = drawer(id_of(87)).await;
    for want in ["INVALID_RECORD (87)", "Kafka's words", "The rule", "What would pass", "cleanup.policy=compact", "kcat -P -K:", "Written", "Nothing", "Produce with a key"] {
        assert!(refused.contains(want), "{want:?} in {refused:.5000}");
    }
    let large = drawer(id_of(10)).await;
    for want in ["MESSAGE_TOO_LARGE (10)", "Batch size is within the limit", "1,048,588", "the broker value applies", "Raise max.message.bytes"] {
        assert!(large.contains(want), "{want:?} in {large:.5000}");
    }
    let fenced_code = tape.rows.iter().find(|r| matches!(r.code, 47 | 90)).map(|r| r.code).expect("a fenced row");
    let fenced = drawer(id_of(fenced_code)).await;
    for want in ["How it came to this", "InitProducerId", "What would pass", "orders-eos"] {
        assert!(fenced.contains(want), "{want:?} in {fenced:.5000}");
    }
    let done = tape.rows.iter().find(|r| r.api == "Fetch").unwrap().id;
    let detail = drawer(done).await;
    for want in ["Fetch v12", "What the broker kept.", "Copy as", "kcat -C", "Correlation id", "confluent-kafka-python"] {
        assert!(detail.contains(want), "{want:?} in {detail:.5000}");
    }
    let (status, gone) = page(&app, get("/live/999999")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(gone.contains("no longer on the wire"));
    let (_, clients) = page(&app, get("/live/clients")).await;
    for want in ["fulfilment", "admin-cli", "confluent-kafka-python", "Only clients that are connected now."] {
        assert!(clients.contains(want), "{want:?} in {clients:.3000}");
    }
    assert!(!clients.contains("<script>alert(1)"));
    w.stop().await;
}

#[tokio::test]
async fn the_docks_the_first_request_and_home_come_from_the_same_tape() {
    let (w, rec) = recorded("docks", false).await;
    traffic(&w).await;
    let app = router(&w, &rec);
    let (status, topic) = page(&app, get("/topics/orders/wire")).await;
    assert_eq!(status, StatusCode::OK);
    for want in ["On the wire: orders", "Collapse to the dock", "orders-0", "Topic and partition"] {
        assert!(topic.contains(want), "{want:?} in {topic:.3000}");
    }
    let (_, group_page) = page(&app, get("/groups/fulfilment/wire")).await;
    assert!(group_page.contains("Every client request that touched fulfilment"), "{group_page:.1500}");
    let (_, first) = page(&app, get("/connect/first-request")).await;
    assert!(first.contains("A new client just connected") && first.contains("sent its first request at"), "{first:.2500}");
    w.stop().await;
}

#[tokio::test]
async fn an_acl_refusal_is_amber_denied_and_not_violet() {
    let (w, rec) = recorded("denied", true).await;
    w.create_topic("orders", 1, &[]).await;
    let mut c = Wire::connect(&w).await;
    c.call(ApiKey::ApiVersions, 3, "intruder", &versions("kcat")).await;
    c.call(ApiKey::Produce, 9, "intruder", &produce("orders", batch(Some("k"), b"v".to_vec(), None), None)).await;
    let app = router(&w, &rec);
    let (_, live) = page(&app, get("/live")).await;
    assert!(live.contains("TOPIC_AUTHORIZATION_FAILED (29)"), "{live:.4000}");
    assert!(live.contains("<span class=\"st warn\">Denied</span>"), "amber, with the word");
    assert!(!live.contains("st refused\">Denied"));
    let id = rec.tape(&WireQuery::default(), kafka_console::source::now_ms()).rows.iter().find(|r| r.code == 29).unwrap().id;
    let (_, d) = page(&app, get(&format!("/live/{id}"))).await;
    for want in ["TOPIC_AUTHORIZATION_FAILED (29)", "An ACL allows this", "Add an ACL for the principal on the topic", "callout warn"] {
        assert!(d.contains(want), "{want:?} in {d:.4000}");
    }
    w.stop().await;
}

#[tokio::test]
async fn a_console_without_a_recorder_says_the_wire_is_empty_and_the_broker_without_an_observer_is_unchanged() {
    // No observer installed: the broker serves, nothing is recorded, the page is honest.
    let w = world("none", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    let mut c = Wire::connect(&w).await;
    let answer = c.call(ApiKey::ApiVersions, 3, "app", &versions("sarama")).await;
    assert!(answer.len() > 20, "the broker answers as it always did");
    let live = Live::new(w.backend.clone(), w.facts.clone());
    let state = State::new(Source::Live(Box::new(live)), Base::root(), vec![], CancellationToken::new());
    let app = kafka_console::app::router(state);
    let (status, live) = page(&app, get("/live")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(live.contains("Nothing has reached the broker"), "{live:.1500}");
    w.stop().await;
}

fn live_dir() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), std::path::PathBuf::from);
    target.join("board-diff-live")
}

/// A page, asserted a 200, written for the live board-diff under the board's id.
async fn board(app: &Router, path: &str, id: &str) -> String {
    let (status, html) = page(app, get(path)).await;
    assert_eq!(status, StatusCode::OK, "{path}: {html:.400}");
    let dir = live_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{id}.html")), &html).unwrap();
    html
}

#[tokio::test]
async fn the_live_render_of_the_boards_over_a_real_broker() {
    // Quiet: nothing has happened yet.
    let (w, rec) = recorded("render-quiet", false).await;
    let quiet = board(&router(&w, &rec), "/live", "live-quiet").await;
    assert!(quiet.contains("No requests yet"));
    w.stop().await;
    // The tape, its drawers, the clients, the docks and the first request.
    let (w, rec) = recorded("render", false).await;
    traffic(&w).await;
    let mut g = Wire::connect(&w).await;
    let mut join = JoinGroupRequest::default();
    join.group_id = GroupId(s("fulfilment"));
    join.session_timeout_ms = 10_000;
    join.rebalance_timeout_ms = 10_000;
    join.protocol_type = s("consumer");
    g.call(ApiKey::JoinGroup, 5, "fulfilment", &join).await;
    let app = router(&w, &rec);
    let wire = board(&app, "/live", "live-wire").await;
    assert!(wire.contains("Refused among the rows kept"));
    let tape = rec.tape(&WireQuery { hide_idle: false, limit: 100, ..WireQuery::default() }, kafka_console::source::now_ms());
    let id = |f: &dyn Fn(&kafka_console::model::WireRow) -> bool| tape.rows.iter().find(|r| f(r)).unwrap().id;
    board(&app, &format!("/live?until={}", id(&|r| r.api == "Metadata")), "live-paused").await;
    board(&app, &format!("/live/{}", id(&|r| r.api == "Fetch")), "request-detail").await;
    board(&app, &format!("/live/{}", id(&|r| r.code == 87)), "request-refused").await;
    board(&app, &format!("/live/{}", id(&|r| r.code == 10)), "request-too-large").await;
    board(&app, &format!("/live/{}", id(&|r| matches!(r.code, 47 | 90))), "request-fenced").await;
    board(&app, "/live/clients", "live-clients").await;
    board(&app, "/connect/first-request", "connect-first-request").await;
    board(&app, "/topics/orders/wire", "topic-wire").await;
    board(&app, "/groups/fulfilment/wire", "group-wire").await;
    w.stop().await;
    // A busy minute: a stream of produces folds into one row.
    let (w, rec) = recorded("render-busy", false).await;
    w.create_topic("clickstream", 1, &[]).await;
    let mut c = Wire::connect(&w).await;
    c.call(ApiKey::ApiVersions, 3, "notifications", &versions("kafkajs")).await;
    for i in 0..150 {
        c.call(ApiKey::Produce, 9, "notifications", &produce("clickstream", batch(Some("k"), format!("v{i}").into_bytes(), None), None)).await;
    }
    let busy = board(&router(&w, &rec), "/live", "live-load").await;
    assert!(busy.contains("A busy minute: 15") && busy.contains("× 150"), "{busy:.3000}");
    w.stop().await;
}
