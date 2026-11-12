//! The planted-secret gate (Phase 8) extended to the recorder (slice 9e): a real broker with SASL on, real clients that log in
//! with a planted password, fail a login with another, produce a record whose key, value and header are planted, change a secret
//! config and a SCRAM credential, and then every place the recorder's knowledge can surface is searched for the plants: the
//! recorder's own Debug, every row and every detail, every page of Live, the drawers, the docks, the clients and the first
//! request.
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
use kafka_console::base::Base;
use kafka_console::model::WireQuery;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_console::Recorder;
use kafka_protocol::protocol::Encodable;
use kafka_wire::StrBytes;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::messages::alter_user_scram_credentials_request::ScramCredentialUpsertion;
use kafka_wire::messages::incremental_alter_configs_request::{AlterConfigsResource, AlterableConfig};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::*;
use kafka_wire::testing::{framed, request_payload};
use support::get;
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
    async fn call<R: Encodable>(&mut self, key: ApiKey, version: i16, client: &str, body: &R) -> Bytes {
        self.1 += 1;
        self.0.write_all(&framed(&request_payload(key, version, self.1, Some(client), body))).await.unwrap();
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
        let auth = Authenticator::new(&[Mechanism::Plain], Credentials::new(users).unwrap()).unwrap();
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
    p.records = Some(BatchBuilder::from_records(1_700_000_000_000, &[rec]).last_offset_delta(0).build());
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
    c.call(ApiKey::IncrementalAlterConfigs, 1, "alice-app", &alter).await;
    let mut up = ScramCredentialUpsertion::default();
    up.name = s("carol");
    up.salt = Bytes::from(SALTED.to_owned());
    up.salted_password = Bytes::from(SALTED.to_owned());
    let mut scram = AlterUserScramCredentialsRequest::default();
    scram.upsertions = vec![up];
    c.call(ApiKey::AlterUserScramCredentials, 0, "alice-app", &scram).await;

    // The corpus: everything the recorder can say.
    let live = Live::new(w.backend.clone(), w.facts.clone()).with_recorder(Arc::clone(&recorder));
    let state = State::new(Source::Live(Box::new(live)), Base::root(), vec![], CancellationToken::new());
    let app = kafka_console::app::router(state);
    let now = kafka_console::source::now_ms();
    let tape = recorder.tape(&WireQuery { hide_idle: false, limit: 100, ..WireQuery::default() }, now);
    assert!(tape.rows.iter().any(|r| r.api == "SaslAuthenticate"), "the login steps are on the tape: {:?}", tape.rows.iter().map(|r| r.api).collect::<Vec<_>>());
    assert!(tape.rows.iter().any(|r| r.api == "AlterUserScramCredentials") && tape.rows.iter().any(|r| r.api == "IncrementalAlterConfigs"));
    let mut corpus = vec![format!("{recorder:?}"), format!("{tape:?}"), format!("{:?}", recorder.clients(now)), format!("{:?}", recorder.first()), format!("{:?}", recorder.spoke())];
    for row in &tape.rows {
        corpus.push(format!("{:?}", recorder.get(row.id)));
        let (_, html) = page(&app, get(&format!("/live/{}", row.id))).await;
        corpus.push(html);
    }
    let mut paths = vec!["/live?idle=0".to_owned(), "/live".to_owned(), "/live/clients".to_owned(), "/connect/first-request".to_owned(), "/topics/orders/wire".to_owned(), "/groups/g/wire".to_owned(), "/".to_owned()];
    for id in ["live-wire", "topic-wire", "group-wire", "home-live"] {
        paths.push(format!("/fragments/{id}?topic=orders&group=g"));
    }
    for path in &paths {
        let (_, html) = page(&app, get(path)).await;
        corpus.push(html);
    }
    // The stream carries fragment ids and nothing else (its own test); the pages above are what it makes the browser fetch.
    for plant in [PASSWORD, WRONG, VALUE, KEY, HEADER, CONFIG, SALTED] {
        for (i, text) in corpus.iter().enumerate() {
            assert!(!text.contains(plant), "the plant {plant:?} reached item {i}: {:.600}", text.replace(plant, "<<PLANT>>"));
        }
    }
    w.stop().await;
}
