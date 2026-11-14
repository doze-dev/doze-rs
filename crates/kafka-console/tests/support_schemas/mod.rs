//! Shared by the live tests of the Schemas API (slice 10d): the console over a world with a registry in the same process,
//! JSON POSTs, drafts, and a record produced with given value bytes.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::sync::Arc;

use crate::support::{HOST, send};
use crate::support_write::World;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use kafka_console::base::Base;
use kafka_console::limits::Limits;
use kafka_console::schemas::Schemas;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_registry::Registry;
use kafka_registry::store::MemoryStorage;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub fn with_registry(w: &World) -> (Router, Arc<Registry>) {
    let registry = Arc::new(Registry::open(Box::new(MemoryStorage::default())).unwrap());
    let live = Live::new(w.backend.clone(), w.facts.clone()).with_schemas(Schemas::On {
        registry: Arc::clone(&registry),
        url: "http://127.0.0.1:18081".to_owned(),
        basic_auth: false,
    });
    let app = kafka_console::app::console(State::console(
        Source::Live(Box::new(live)),
        &Base::root(),
        vec![],
        CancellationToken::new(),
        &Limits::default(),
    ));
    (app, registry)
}

pub async fn post_json(app: &Router, path: &str, body: &Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header("x-console", "1")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let (res, text) = send(app, req).await;
    (
        res.status(),
        serde_json::from_str(&text).unwrap_or(Value::Null),
    )
}

pub const V1: &str =
    r#"{"type":"record","name":"Order","namespace":"shop","fields":[{"name":"id","type":"long"}]}"#;
pub const V2: &str = r#"{"type":"record","name":"Order","namespace":"shop","fields":[{"name":"id","type":"long"},{"name":"note","type":["null","string"],"default":null}]}"#;
pub const V3_BAD: &str = r#"{"type":"record","name":"Order","namespace":"shop","fields":[{"name":"id","type":"long"},{"name":"qty","type":"int"}]}"#;

pub fn draft(subject: &str, kind: &str, schema: &str) -> Value {
    json!({"subject": subject, "schema_type": kind, "schema": schema})
}

pub async fn produce_value(w: &World, topic: &str, value: Vec<u8>) {
    let batch = BatchBuilder::from_records(
        1_000,
        &[Rec {
            key: Some(b"k".to_vec()),
            value: Some(value),
            ..Rec::default()
        }],
    )
    .build();
    assert_eq!(w.produce_batch(topic, 0, batch, None).await.0, 0);
}
