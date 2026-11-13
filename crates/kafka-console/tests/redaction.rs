//! The planted-secret gate (Phase 8) extended to the recorder (slice 9e): a real broker with SASL on, real clients that log in
//! with a planted password, fail a login with another, produce a record whose key, value and header are planted, change a secret
//! config and a SCRAM credential, and then every place the recorder's knowledge can surface is searched for the plants: the
//! recorder's own Debug, every row and every detail, and every JSON answer of the console's API (every request in full, the
//! Live tape, the docks, the clients and the first request among them).
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

use bytes::Bytes;
use kafka_auth::{Authenticator, Credentials, Mechanism};
use kafka_console::Recorder;
use kafka_console::base::Base;
use kafka_console::model::WireQuery;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_protocol::protocol::Encodable;
use kafka_wire::StrBytes;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::messages::alter_user_scram_credentials_request::ScramCredentialUpsertion;
use kafka_wire::messages::incremental_alter_configs_request::{
    AlterConfigsResource, AlterableConfig,
};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::*;
use kafka_wire::testing::{framed, request_payload};
use support_write::{page, world};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

const PASSWORD: &str = "alice-pw-planted-7f3a91";
const WRONG: &str = "wrong-pw-planted-55aa02";
const VALUE: &str = "record-value-planted-9d1c44";
const KEY: &str = "record-key-planted-2be7f0";
const HEADER: &str = "record-header-planted-c0ffee";
const CONFIG: &str = "config-secret-planted-3c8d15";
const SALTED: &str = "salted-password-planted-1a2b3c";

struct Wire(TcpStream, i32);

impl Wire {
    async fn call<R: Encodable>(
        &mut self,
        key: ApiKey,
        version: i16,
        client: &str,
        body: &R,
    ) -> Bytes {
        self.1 += 1;
        self.0
            .write_all(&framed(&request_payload(
                key,
                version,
                self.1,
                Some(client),
                body,
            )))
            .await
            .unwrap();
        let mut len = [0; 4];
        self.0.read_exact(&mut len).await.unwrap();
        let mut payload = vec![0; u32::from_be_bytes(len) as usize];
        self.0.read_exact(&mut payload).await.unwrap();
        Bytes::from(payload)
    }

    async fn log_in(&mut self, password: &str) {
        let mut hs = SaslHandshakeRequest::default();
        hs.mechanism = StrBytes::from_static_str("PLAIN");
        self.call(ApiKey::SaslHandshake, 1, "svc", &hs).await;
        let mut auth = SaslAuthenticateRequest::default();
        auth.auth_bytes = Bytes::from(format!("\0alice\0{password}"));
        self.call(ApiKey::SaslAuthenticate, 2, "svc", &auth).await;
    }
}

fn s(t: &str) -> StrBytes {
    StrBytes::from_string(t.to_owned())
}

#[tokio::test]
async fn no_planted_secret_reaches_the_recorder_the_stream_or_a_page() {
    let recorder = Recorder::new();
    let r = Arc::clone(&recorder);
    let w = world("redaction", move |_, sc| {
        let users = [("alice".to_owned(), PASSWORD.to_owned())];
        let auth =
            Authenticator::new(&[Mechanism::Plain], Credentials::new(users).unwrap()).unwrap();
        sc.sasl = Some(Arc::new(auth));
        sc.observer = Some(r);
    })
    .await;
    w.create_topic("orders", 1, &[]).await;
    let addr = w.server.as_ref().unwrap().local_addr();
    // A login that fails, with a plant for the wrong password.
    let mut bad = Wire(TcpStream::connect(addr).await.unwrap(), 0);
    bad.log_in(WRONG).await;
    // A login that works, then work whose requests carry plants.
    let mut c = Wire(TcpStream::connect(addr).await.unwrap(), 0);
    c.log_in(PASSWORD).await;
    let rec = Rec {
        key: Some(KEY.as_bytes().to_vec()),
        value: Some(VALUE.as_bytes().to_vec()),
        headers: vec![(b"trace".to_vec(), HEADER.as_bytes().to_vec())],
        ..Rec::default()
    };
    let mut p = PartitionProduceData::default();
    p.records = Some(
        BatchBuilder::from_records(1_700_000_000_000, &[rec])
            .last_offset_delta(0)
            .build(),
    );
    let mut t = TopicProduceData::default();
    t.name = TopicName(s("orders"));
    t.partition_data = vec![p];
    let mut produce = ProduceRequest::default();
    produce.acks = 1;
    produce.topic_data = vec![t];
    c.call(ApiKey::Produce, 9, "alice-app", &produce).await;
    let mut cfg = AlterableConfig::default();
    cfg.name = s("ssl.key.password");
    cfg.value = Some(s(CONFIG));
    let mut res = AlterConfigsResource::default();
    res.resource_type = 4;
    res.resource_name = s("0");
    res.configs = vec![cfg];
    let mut alter = IncrementalAlterConfigsRequest::default();
    alter.resources = vec![res];
    c.call(ApiKey::IncrementalAlterConfigs, 1, "alice-app", &alter)
        .await;
    let mut up = ScramCredentialUpsertion::default();
    up.name = s("carol");
    up.salt = Bytes::from(SALTED.to_owned());
    up.salted_password = Bytes::from(SALTED.to_owned());
    let mut scram = AlterUserScramCredentialsRequest::default();
    scram.upsertions = vec![up];
    c.call(ApiKey::AlterUserScramCredentials, 0, "alice-app", &scram)
        .await;

    // The corpus: everything the recorder can say.
    let live = Live::new(w.backend.clone(), w.facts.clone()).with_recorder(Arc::clone(&recorder));
    let state = State::new(
        Source::Live(Box::new(live)),
        Base::root(),
        vec![],
        CancellationToken::new(),
    );
    let app = kafka_console::app::console(state);
    let now = kafka_console::source::now_ms();
    let tape = recorder.tape(
        &WireQuery {
            hide_idle: false,
            limit: 100,
            ..WireQuery::default()
        },
        now,
    );
    assert!(
        tape.rows.iter().any(|r| r.api == "SaslAuthenticate"),
        "the login steps are on the tape: {:?}",
        tape.rows.iter().map(|r| r.api).collect::<Vec<_>>()
    );
    assert!(
        tape.rows
            .iter()
            .any(|r| r.api == "AlterUserScramCredentials")
            && tape.rows.iter().any(|r| r.api == "IncrementalAlterConfigs")
    );
    let mut corpus = vec![
        format!("{recorder:?}"),
        format!("{tape:?}"),
        format!("{:?}", recorder.clients(now)),
        format!("{:?}", recorder.first()),
        format!("{:?}", recorder.spoke()),
    ];
    for row in &tape.rows {
        corpus.push(format!("{:?}", recorder.get(row.id)));
        let (_, html) = page(
            &app,
            support::api_get(&format!("/api/v1/live/request/{}", row.id)),
        )
        .await;
        corpus.push(html);
    }
    // Every GET endpoint of the API, over the same broker and recorder.
    for path in kafka_console::api::GET_PATHS {
        let req = axum::http::Request::builder()
            .uri(*path)
            .header(axum::http::header::HOST, support::HOST)
            .header("x-console", "1")
            .body(axum::body::Body::empty())
            .unwrap();
        let (status, json) = page(&app, req).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{path}: {json}");
        corpus.push(json);
    }
    // The GETs of one group (over groups this broker has: commits make them), the Lab's GETs with a rule in force, a
    // representative POST of every write (made, checked and refused), and the first frames of the live tail (audit finding 4).
    for group in ["search-indexer", "analytics", "fulfilment"] {
        c.call(
            ApiKey::OffsetCommit,
            8,
            "alice-app",
            &commit(group, "orders"),
        )
        .await;
    }
    let lab_app = {
        let live = Live::new(w.backend.clone(), w.facts.clone())
            .with_recorder(Arc::clone(&recorder))
            .with_lab(kafka_server::fault::FaultTable::new());
        kafka_console::app::console(State::new(
            Source::Live(Box::new(live)),
            Base::root(),
            vec![],
            CancellationToken::new(),
        ))
    };
    let (status, made) = page(
        &lab_app,
        json_post(
            "/api/v1/lab/rules",
            &serde_json::json!({"kind": "slow", "fields": {"api": "Produce", "topic": "orders", "client": "alice-app", "delay": "5", "ends": "5"}}),
        ),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{made}");
    let rule = serde_json::from_str::<serde_json::Value>(&made).unwrap()["id"]
        .as_u64()
        .unwrap_or(1);
    corpus.push(made);
    let lab_gets = [
        "/api/v1/lab".to_owned(),
        "/api/v1/lab/form?kind=slow".to_owned(),
        "/api/v1/lab/form?kind=fail".to_owned(),
        format!("/api/v1/lab/rules/{rule}"),
        "/api/v1/lab/once/drop".to_owned(),
        "/api/v1/lab/once/rebalance".to_owned(),
        "/api/v1/lab/compact?topic=orders&partition=0".to_owned(),
    ];
    let group_gets = kafka_console::api::groups::GROUP_GET_PATHS
        .iter()
        .map(|p| (*p).to_owned());
    for path in group_gets.chain(lab_gets) {
        let (status, json) = page(&lab_app, support::api_get(&path)).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{path}: {json}");
        corpus.push(json);
    }
    let form = |path: &str, fields: &[(&str, &str)]| {
        let mut req = support::post(path, &support_write::body(fields));
        req.headers_mut()
            .insert("x-console", axum::http::HeaderValue::from_static("1"));
        req
    };
    let writes = [
        json_post("/api/v1/probe", &serde_json::json!({})),
        form(
            "/api/v1/create-topic",
            &[("name", "fresh"), ("partitions", "1"), ("mode", "create")],
        ),
        // Refused: the name is taken.
        form(
            "/api/v1/create-topic",
            &[("name", "orders"), ("partitions", "1"), ("mode", "create")],
        ),
        form(
            "/api/v1/topics/fresh/produce",
            &[
                ("partition", "0"),
                ("key", "k"),
                ("value", "v"),
                ("acks", "-1"),
            ],
        ),
        // Refused: no such topic.
        form("/api/v1/topics/nowhere/produce", &[("value", "v")]),
        form("/api/v1/topics/fresh/add-partitions", &[("count", "2")]),
        form("/api/v1/topics/fresh/add-partitions", &[("count", "1")]),
        form(
            "/api/v1/topics/fresh/delete-records",
            &[("partition", "0"), ("offset", "1")],
        ),
        form(
            "/api/v1/topics/fresh/config/edit",
            &[("key", "retention.ms"), ("op", "set"), ("value", "7200000")],
        ),
        form(
            "/api/v1/topics/fresh/config/reset",
            &[("key", "retention.ms")],
        ),
        json_post(
            "/api/v1/topics/orders/log/compact?partition=0",
            &serde_json::json!({}),
        ),
        json_post(
            "/api/v1/groups/analytics/reset",
            &serde_json::json!({"topic": "orders", "to": "earliest", "value": ""}),
        ),
        json_post(
            "/api/v1/groups/analytics/offsets/delete",
            &serde_json::json!({"topics": ["orders"]}),
        ),
        json_post("/api/v1/groups/fulfilment/delete", &serde_json::json!({})),
        json_post("/api/v1/groups/nobody/delete", &serde_json::json!({})),
        json_post(
            "/api/v1/settings/edit",
            &serde_json::json!({"key": "log.retention.ms", "scope": "all", "value": "86400000", "check": true}),
        ),
        // A sensitive broker config typed in is never said back (a value typed for an ordinary key, refused, is quoted
        // in Kafka's sentence as Kafka does: not a secret).
        json_post(
            "/api/v1/settings/edit",
            &serde_json::json!({"key": "ssl.key.password", "scope": "all", "value": CONFIG}),
        ),
        json_post(
            "/api/v1/access/acls/new",
            &serde_json::json!({"type": "topic", "name": "orders", "pattern": "literal", "principal": "User:alice", "host": "*", "op": "read", "perm": "allow"}),
        ),
        json_post(
            "/api/v1/access/acls/new",
            &serde_json::json!({"type": "topic", "principal": "User:x"}),
        ),
        json_post(
            "/api/v1/access/acls/delete",
            &serde_json::json!({"type": "topic", "name": "orders", "pattern": "literal", "principal": "User:alice", "host": "*", "op": "read", "perm": "allow"}),
        ),
        json_post(
            &format!("/api/v1/lab/rules/{rule}/enabled"),
            &serde_json::json!({"on": false}),
        ),
        json_post(
            "/api/v1/lab/rules",
            &serde_json::json!({"kind": "slow", "fields": {"delay": "0"}}),
        ),
        json_post(
            "/api/v1/lab/once/drop",
            &serde_json::json!({"target": "nobody", "how": "close"}),
        ),
        json_post(
            "/api/v1/lab/compact",
            &serde_json::json!({"topic": "orders", "partition": 0}),
        ),
        json_post(
            &format!("/api/v1/lab/rules/{rule}/remove"),
            &serde_json::json!({}),
        ),
        json_post("/api/v1/lab/history/clear", &serde_json::json!({})),
        // Last: the topic goes.
        form("/api/v1/topics/fresh/delete", &[("confirm", "fresh")]),
    ];
    for req in writes {
        let path = req.uri().to_string();
        let (status, json) = page(&lab_app, req).await;
        assert!(
            status.as_u16() < 500,
            "{path} answered {status}: {json:.300}"
        );
        assert!(
            json.starts_with('{'),
            "{path} answered {status}: {json:.300}"
        );
        corpus.push(json);
    }
    // The live tail from the start of this process's tape: its first frames carry every row the recorder has.
    corpus.push(first_frames(&lab_app, "/api/v1/tail/live?idle=0&pace=1").await);
    let tail = corpus.last().unwrap();
    assert!(
        tail.contains("SaslAuthenticate"),
        "the tail sent the rows: {tail:.600}"
    );
    for plant in [PASSWORD, WRONG, VALUE, KEY, HEADER, CONFIG, SALTED] {
        for (i, text) in corpus.iter().enumerate() {
            assert!(
                !text.contains(plant),
                "the plant {plant:?} reached item {i}: {:.600}",
                text.replace(plant, "<<PLANT>>")
            );
        }
    }
    // The messages tail shows records (that is its job), so only the secrets that are not a record's are looked for.
    let messages = first_frames(&lab_app, "/api/v1/tail/messages?topic=orders&pace=1").await;
    for plant in [PASSWORD, WRONG, CONFIG, SALTED] {
        assert!(
            !messages.contains(plant),
            "{plant:?} reached the messages tail"
        );
    }
    w.stop().await;
}

/// A POST of JSON, as the React app's fetch client sends it.
fn json_post(path: &str, body: &serde_json::Value) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header(axum::http::header::HOST, support::HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

/// A tail from the first cursor of this process (`Last-Event-ID`): what it sends in its first second.
async fn first_frames(app: &axum::Router, path: &str) -> String {
    use http_body_util::BodyExt;
    let mut req = support::api_get(path);
    req.headers_mut().insert(
        "last-event-id",
        kafka_console::api::live::cursor(0).parse().unwrap(),
    );
    let res = support::head(app, req).await;
    assert_eq!(res.status(), axum::http::StatusCode::OK, "{path}");
    let mut body = res.into_body();
    let mut out = String::new();
    let until = tokio::time::Instant::now() + std::time::Duration::from_millis(1500);
    while let Ok(Some(Ok(frame))) = tokio::time::timeout_at(until, body.frame()).await {
        if let Some(data) = frame.data_ref() {
            out.push_str(&String::from_utf8_lossy(data));
        }
    }
    out
}

fn commit(group: &str, topic: &str) -> OffsetCommitRequest {
    use kafka_wire::messages::offset_commit_request::{
        OffsetCommitRequestPartition, OffsetCommitRequestTopic,
    };
    let mut p = OffsetCommitRequestPartition::default();
    p.partition_index = 0;
    p.committed_offset = 0;
    p.committed_leader_epoch = -1;
    let mut t = OffsetCommitRequestTopic::default();
    t.name = TopicName(s(topic));
    t.partitions = vec![p];
    let mut c = OffsetCommitRequest::default();
    c.group_id = GroupId(s(group));
    c.generation_id_or_member_epoch = -1;
    c.topics = vec![t];
    c
}
