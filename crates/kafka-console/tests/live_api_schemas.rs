//! Slice 10d: the Schemas pages' JSON API over a real broker and a registry in the same process, and the schema parts of the
//! topic pages: records in the registry's wire format decoded (Avro, JSON Schema, Protobuf), a value encoded and produced,
//! the topics list's subject column, the topic's Schema tab, Connect's registry lines and the sidebar's count.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_api;
mod support_log;
mod support_write;

use std::sync::Arc;

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
use support::{HOST, send};
use support_api::{api, get, post};
use support_write::{World, world};
use tokio_util::sync::CancellationToken;

fn with_registry(w: &World) -> (Router, Arc<Registry>) {
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

async fn post_json(app: &Router, path: &str, body: &Value) -> (StatusCode, Value) {
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
    (res.status(), serde_json::from_str(&text).unwrap_or(Value::Null))
}

const V1: &str = r#"{"type":"record","name":"Order","namespace":"shop","fields":[{"name":"id","type":"long"}]}"#;
const V2: &str = r#"{"type":"record","name":"Order","namespace":"shop","fields":[{"name":"id","type":"long"},{"name":"note","type":["null","string"],"default":null}]}"#;
const V3_BAD: &str = r#"{"type":"record","name":"Order","namespace":"shop","fields":[{"name":"id","type":"long"},{"name":"qty","type":"int"}]}"#;

fn draft(subject: &str, kind: &str, schema: &str) -> Value {
    json!({"subject": subject, "schema_type": kind, "schema": schema})
}

async fn produce_value(w: &World, topic: &str, value: Vec<u8>) {
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

/// CON-1500: with no registry the list says why and every other Schemas call is SCHEMAS_OFF; the sidebar has no count;
/// Connect has no registry lines.
#[tokio::test]
async fn without_a_registry_the_pages_say_why() {
    let w = world("schemas-off", |_, _| {}).await;
    let app = api(&w);
    let (s, v) = get(&app, "/api/v1/schemas").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["off"]["title"], "Not in this build");
    assert!(v["off"]["command"].as_str().unwrap().ends_with("--schema-registry"), "{v}");
    let (s, v) = get(&app, "/api/v1/schemas/subjects/orders-value").await;
    assert_eq!((s, v["code"].as_str()), (StatusCode::CONFLICT, Some("SCHEMAS_OFF")));
    assert!(get(&app, "/api/v1/shell").await.1["schemas"].is_null());
    assert!(get(&app, "/api/v1/connect").await.1["registry"].is_null());
    let (_, v) = post_json(&app, "/api/v1/schemas/register", &draft("x", "AVRO", V1)).await;
    assert_eq!(v["code"], "SCHEMAS_OFF");
    w.stop().await;
}

/// CON-1501 to CON-1505: register, list (with the topic its name points at), a subject's page, the live check with its
/// reasons in Confluent's words and Go's one fix, a refused registration (409) with the same reasons, the diff and the
/// verdict at each level, levels and modes, soft and permanent deletes with the typed confirmation.
#[tokio::test]
async fn subjects_are_registered_checked_diffed_configured_and_deleted() {
    let w = world("schemas-api", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    let (app, _) = with_registry(&w);
    let (_, v) = post_json(&app, "/api/v1/schemas/check", &draft("orders-value", "AVRO", V1)).await;
    assert_eq!(v["outcome"], "first", "{v}");
    let (_, v) = post_json(&app, "/api/v1/schemas/register", &draft("orders-value", "AVRO", V1)).await;
    assert_eq!((v["outcome"].as_str(), v["version"].as_u64(), v["existed"].as_bool()), (Some("registered"), Some(1), Some(false)), "{v}");
    let (_, v) = post_json(&app, "/api/v1/schemas/register", &draft("orders-value", "AVRO", V1)).await;
    assert_eq!(v["existed"], true, "the same schema again creates nothing");
    let (_, v) = post_json(&app, "/api/v1/schemas/check", &draft("orders-value", "AVRO", V2)).await;
    assert_eq!((v["outcome"].as_str(), v["level"].as_str()), (Some("compatible"), Some("BACKWARD")), "{v}");
    let (_, v) = post_json(&app, "/api/v1/schemas/check", &draft("orders-value", "AVRO", V3_BAD)).await;
    assert_eq!(v["outcome"], "incompatible", "{v}");
    let reason = &v["findings"][0]["reasons"][0];
    assert_eq!(reason["kind"], "READER_FIELD_MISSING_DEFAULT_VALUE");
    assert_eq!(
        reason["description"],
        "The field 'qty' at path '/fields/1' in the new schema has no default value and is missing in the old schema"
    );
    assert_eq!(reason["fix"]["label"], "Add \"default\": 0 to qty");
    let fixed = reason["fix"]["schema"].as_str().unwrap().to_owned();
    let (_, v) = post_json(&app, "/api/v1/schemas/check", &draft("orders-value", "AVRO", &fixed)).await;
    assert_eq!(v["outcome"], "compatible", "the fix makes it pass: {v}");
    let (_, v) = post_json(&app, "/api/v1/schemas/register", &draft("orders-value", "AVRO", V3_BAD)).await;
    assert_eq!((v["outcome"].as_str(), v["status"].as_u64(), v["error_code"].as_u64()), (Some("refused"), Some(409), Some(409)), "{v}");
    assert_eq!(v["findings"][0]["reasons"][0]["kind"], "READER_FIELD_MISSING_DEFAULT_VALUE");
    let (_, v) = post_json(&app, "/api/v1/schemas/check", &draft("orders-value", "AVRO", "{nope")).await;
    assert_eq!((v["outcome"].as_str(), v["error_code"].as_u64()), (Some("invalid"), Some(42201)), "{v}");
    post_json(&app, "/api/v1/schemas/register", &draft("orders-value", "AVRO", V2)).await;

    let (_, v) = get(&app, "/api/v1/schemas").await;
    assert!(v["off"].is_null());
    let row = &v["subjects"][0];
    assert_eq!((row["subject"].as_str(), row["kind"].as_str(), row["versions"].as_u64()), (Some("orders-value"), Some("AVRO"), Some(2)));
    assert_eq!(row["topics"], json!([["orders", "value"]]));
    assert_eq!(get(&app, "/api/v1/shell").await.1["schemas"], 1);
    assert_eq!(get(&app, "/api/v1/topics").await.1["subjects"], json!([["orders", "orders-value"]]));
    assert_eq!(get(&app, "/api/v1/connect").await.1["registry"]["properties"], "schema.registry.url=http://127.0.0.1:18081");

    let (_, v) = get(&app, "/api/v1/schemas/subjects/orders-value").await;
    assert_eq!(v["shown"]["version"], 2);
    assert_eq!(v["shown"]["schema"], "{\n  \"type\": \"record\",\n  \"name\": \"Order\",\n  \"namespace\": \"shop\",\n  \"fields\": [\n    {\"name\": \"id\", \"type\": \"long\"},\n    {\"name\": \"note\", \"type\": [\"null\", \"string\"], \"default\": null}\n  ]\n}");
    assert_eq!(v["versions"][0]["registered_by"], "this console");
    assert_eq!(v["shown"]["sample"], "{\n  \"id\": 0,\n  \"note\": null\n}");
    let (_, v) = get(&app, "/api/v1/schemas/subjects/orders-value?version=1").await;
    assert_eq!(v["shown"]["version"], 1);
    let (_, v) = get(&app, "/api/v1/schemas/subjects/nope").await;
    assert_eq!(v["missing"]["code"], "SUBJECT_NOT_FOUND");

    let (_, v) = get(&app, "/api/v1/schemas/subjects/orders-value/diff").await;
    // One field a line: the new field is one added line, and the line before it gains a comma.
    assert_eq!((v["from"].as_u64(), v["to"].as_u64(), v["removed"].as_u64(), v["added"].as_u64()), (Some(1), Some(2), Some(1), Some(2)), "{v}");
    let checks = v["checks"].as_array().unwrap();
    assert!(checks.iter().all(|c| c["ok"] == true), "a nullable field with a default passes all three: {v}");

    let (_, v) = post_json(&app, "/api/v1/schemas/subjects/orders-value/config", &json!({"level": "NONE"})).await;
    assert_eq!((v["outcome"].as_str(), v["level"].as_str()), (Some("configured"), Some("NONE")));
    let (_, v) = post_json(&app, "/api/v1/schemas/subjects/orders-value/config", &json!({"level": "default", "mode": "READONLY"})).await;
    assert_eq!((v["level"].as_str(), v["mode"].as_str()), (Some("BACKWARD"), Some("READONLY")));
    let (_, v) = post_json(&app, "/api/v1/schemas/subjects/orders-value/delete", &json!({"version": 1})).await;
    assert_eq!((v["outcome"].as_str(), v["error_code"].as_u64()), (Some("refused"), Some(42205)), "read-only refuses: {v}");
    post_json(&app, "/api/v1/schemas/subjects/orders-value/config", &json!({"mode": "READWRITE"})).await;
    let (_, v) = post_json(&app, "/api/v1/schemas/subjects/orders-value/delete", &json!({"version": 1, "permanent": true})).await;
    assert_eq!(v["error_code"], 40407, "permanent before soft is refused in Confluent's code: {v}");
    let (_, v) = post_json(&app, "/api/v1/schemas/subjects/orders-value/delete", &json!({"version": 1})).await;
    assert_eq!((v["outcome"].as_str(), v["versions"].clone()), (Some("deleted"), json!([1])));
    let (s, v) = post_json(&app, "/api/v1/schemas/subjects/orders-value/delete", &json!({"permanent": true})).await;
    assert_eq!((s, v["code"].as_str()), (StatusCode::BAD_REQUEST, Some("CONFIRM")));
    post_json(&app, "/api/v1/schemas/subjects/orders-value/delete", &json!({})).await;
    let (_, v) = get(&app, "/api/v1/schemas").await;
    assert_eq!(v["subjects"][0]["deleted"], true, "a soft-deleted subject is listed as such");
    let (_, v) = post_json(&app, "/api/v1/schemas/subjects/orders-value/delete", &json!({"permanent": true, "confirm": "orders-value"})).await;
    assert_eq!(v["outcome"], "deleted", "{v}");
    assert_eq!(get(&app, "/api/v1/schemas").await.1["subjects"], json!([]));
    w.stop().await;
}

const JSON_SCHEMA: &str = r#"{"type":"object","properties":{"sku":{"type":"string"},"qty":{"type":"integer","minimum":1}},"required":["sku"]}"#;
const PROTO: &str = "syntax = \"proto3\";\npackage shop;\nmessage Payment { string id = 1; int64 cents = 2; }\n";

/// CON-1510 to CON-1513: records framed with the wire header are decoded with their schema in the table and the Sheet
/// (Avro, JSON Schema, Protobuf), with the subject and version that hold the id; an unknown id and bytes that do not fit say
/// why; Try encodes without sending; Produce encodes, sends, and the record reads back.
#[tokio::test]
async fn records_decode_with_their_schemas_and_produce_encodes() {
    let w = world("schemas-decode", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    let (app, r) = with_registry(&w);
    post_json(&app, "/api/v1/schemas/register", &draft("orders-value", "AVRO", V2)).await;
    post_json(&app, "/api/v1/schemas/register", &draft("stock-value", "JSON", JSON_SCHEMA)).await;
    post_json(&app, "/api/v1/schemas/register", &draft("pay-value", "PROTOBUF", PROTO)).await;
    let avro = r.codec_for("orders-value", "latest").unwrap().0;
    let js = r.codec_for("stock-value", "latest").unwrap().0;
    let pb = r.codec_for("pay-value", "latest").unwrap().0;
    produce_value(&w, "orders", avro.encode(r#"{"id":7,"note":"rush"}"#).unwrap()).await;
    produce_value(&w, "orders", js.encode(r#"{"sku":"A-1","qty":2}"#).unwrap()).await;
    produce_value(&w, "orders", pb.encode(r#"{"id":"p-9","cents":"1250"}"#).unwrap()).await;
    let mut unknown = kafka_registry::wire::header(99).to_vec();
    unknown.extend(b"zz");
    produce_value(&w, "orders", unknown).await;
    let mut broken = kafka_registry::wire::header(avro.id()).to_vec();
    broken.push(0x80);
    produce_value(&w, "orders", broken).await;

    let (_, v) = get(&app, "/api/v1/topics/orders/messages").await;
    let rows = v["view"]["rows"].as_array().unwrap();
    let by_offset = |o: i64| rows.iter().find(|x| x["offset"] == o).unwrap()["value"]["schema"].clone();
    let a = by_offset(0);
    assert_eq!((a["kind"].as_str(), a["subject"].as_str(), a["version"].as_u64()), (Some("AVRO"), Some("orders-value"), Some(1)));
    assert_eq!(a["preview"], r#"{"id":7,"note":{"string":"rush"}}"#);
    assert_eq!(by_offset(1)["preview"], r#"{"sku":"A-1","qty":2}"#);
    assert_eq!(by_offset(2)["preview"], r#"{"id":"p-9","cents":"1250"}"#);
    assert_eq!(by_offset(3)["error"], "Schema id 99 is not registered here (40403).");
    assert!(by_offset(4)["error"].as_str().unwrap().starts_with("the payload does not match the schema"), "{}", by_offset(4));

    let (_, v) = get(&app, "/api/v1/topics/orders/records/0/2").await;
    let p = &v["value"]["schema"];
    assert_eq!((p["kind"].as_str(), p["message"].as_str()), (Some("PROTOBUF"), Some("shop.Payment")));
    assert_eq!(p["json"], "{\n  \"id\": \"p-9\",\n  \"cents\": \"1250\"\n}");
    let (_, v) = get(&app, "/api/v1/topics/orders/records/0/1").await;
    assert!(v["value"]["schema"]["mismatch"].is_null());

    let (_, v) = post_json(&app, "/api/v1/schemas/try", &json!({"subject": "stock-value", "version": "latest", "value": r#"{"qty":0}"#})).await;
    assert_eq!((v["ok"].as_bool(), v["error"].as_str()), (Some(false), Some("#: required property \"sku\" is missing")));
    let (_, v) = post_json(&app, "/api/v1/schemas/try", &json!({"subject": "orders-value", "version": "1", "value": r#"{"id":1}"#})).await;
    assert_eq!((v["ok"].as_bool(), v["size"].as_u64()), (Some(true), Some(7)), "{v}");
    assert!(v["hex"].as_str().unwrap().starts_with("00 00 00 00 01"), "{v}");

    let (_, v) = get(&app, "/api/v1/topics/orders/produce?value=%7B%22id%22%3A5%7D&schema=orders-value").await;
    assert_eq!(v["suggested"], "orders-value");
    assert_eq!((v["encoded"]["ok"].as_bool(), v["encoded"]["kind"].as_str()), (Some(true), Some("AVRO")), "{v}");
    let (_, v) = post(&app, "/api/v1/topics/orders/produce", &[("value", r#"{"id":"x"}"#), ("schema", "orders-value"), ("partition", "0")]).await;
    assert_eq!(v["outcome"], "invalid");
    assert_eq!(v["message"], "the value does not fit orders-value version 1: id: expected a number, not \"x\"");
    let (_, v) = post(&app, "/api/v1/topics/orders/produce", &[("value", r#"{"id":5}"#), ("schema", "orders-value"), ("partition", "0")]).await;
    assert_eq!(v["outcome"], "done", "{v}");
    let (_, v) = get(&app, "/api/v1/topics/orders/records/0/5").await;
    assert_eq!(v["value"]["schema"]["json"], "{\n  \"id\": 5,\n  \"note\": null\n}");

    let (_, v) = get(&app, "/api/v1/topics/orders/schemas").await;
    let subjects: Vec<(&str, &str)> = v["subjects"].as_array().unwrap().iter().map(|x| (x["subject"].as_str().unwrap(), x["position"].as_str().unwrap())).collect();
    assert_eq!(subjects, [("orders-value", "value"), ("pay-value", "carried"), ("stock-value", "carried")]);
    assert_eq!(v["unknown"], json!([99]));
    w.stop().await;
}
