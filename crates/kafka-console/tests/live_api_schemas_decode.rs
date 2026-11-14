//! Slice 10d: records in the registry's wire format decoded (Avro, JSON Schema, Protobuf) in the table, the Sheet and the
//! topic's Schema tab; Try; a value encoded and produced by the Produce dialog.
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
mod support_schemas;
mod support_write;
use serde_json::json;
use support_api::{get, post};
use support_schemas::{V2, draft, post_json, produce_value, with_registry};
use support_write::world;

const JSON_SCHEMA: &str = r#"{"type":"object","properties":{"sku":{"type":"string"},"qty":{"type":"integer","minimum":1}},"required":["sku"]}"#;
const PROTO: &str =
    "syntax = \"proto3\";\npackage shop;\nmessage Payment { string id = 1; int64 cents = 2; }\n";

/// CON-1510 to CON-1513: records framed with the wire header are decoded with their schema in the table and the Sheet
/// (Avro, JSON Schema, Protobuf), with the subject and version that hold the id; an unknown id and bytes that do not fit say
/// why; Try encodes without sending; Produce encodes, sends, and the record reads back.
#[tokio::test]
async fn records_decode_with_their_schemas_and_produce_encodes() {
    let w = world("schemas-decode", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    let (app, r) = with_registry(&w);
    post_json(
        &app,
        "/api/v1/schemas/register",
        &draft("orders-value", "AVRO", V2),
    )
    .await;
    post_json(
        &app,
        "/api/v1/schemas/register",
        &draft("stock-value", "JSON", JSON_SCHEMA),
    )
    .await;
    post_json(
        &app,
        "/api/v1/schemas/register",
        &draft("pay-value", "PROTOBUF", PROTO),
    )
    .await;
    let avro = r.codec_for("orders-value", "latest").unwrap().0;
    let js = r.codec_for("stock-value", "latest").unwrap().0;
    let pb = r.codec_for("pay-value", "latest").unwrap().0;
    produce_value(
        &w,
        "orders",
        avro.encode(r#"{"id":7,"note":"rush"}"#).unwrap(),
    )
    .await;
    produce_value(&w, "orders", js.encode(r#"{"sku":"A-1","qty":2}"#).unwrap()).await;
    produce_value(
        &w,
        "orders",
        pb.encode(r#"{"id":"p-9","cents":"1250"}"#).unwrap(),
    )
    .await;
    let mut unknown = kafka_registry::wire::header(99).to_vec();
    unknown.extend(b"zz");
    produce_value(&w, "orders", unknown).await;
    let mut broken = kafka_registry::wire::header(avro.id()).to_vec();
    broken.push(0x80);
    produce_value(&w, "orders", broken).await;

    let (_, v) = get(&app, "/api/v1/topics/orders/messages").await;
    let rows = v["view"]["rows"].as_array().unwrap();
    let by_offset =
        |o: i64| rows.iter().find(|x| x["offset"] == o).unwrap()["value"]["schema"].clone();
    let a = by_offset(0);
    assert_eq!(
        (
            a["kind"].as_str(),
            a["subject"].as_str(),
            a["version"].as_u64()
        ),
        (Some("AVRO"), Some("orders-value"), Some(1))
    );
    assert_eq!(a["preview"], r#"{"id":7,"note":{"string":"rush"}}"#);
    assert_eq!(by_offset(1)["preview"], r#"{"sku":"A-1","qty":2}"#);
    assert_eq!(by_offset(2)["preview"], r#"{"id":"p-9","cents":"1250"}"#);
    assert_eq!(
        by_offset(3)["error"],
        "Schema id 99 is not registered here (40403)."
    );
    assert!(
        by_offset(4)["error"]
            .as_str()
            .unwrap()
            .starts_with("the payload does not match the schema"),
        "{}",
        by_offset(4)
    );

    let (_, v) = get(&app, "/api/v1/topics/orders/records/0/2").await;
    let p = &v["value"]["schema"];
    assert_eq!(
        (p["kind"].as_str(), p["message"].as_str()),
        (Some("PROTOBUF"), Some("shop.Payment"))
    );
    assert_eq!(p["json"], "{\n  \"id\": \"p-9\",\n  \"cents\": \"1250\"\n}");
    let (_, v) = get(&app, "/api/v1/topics/orders/records/0/1").await;
    assert!(v["value"]["schema"]["mismatch"].is_null());

    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/try",
        &json!({"subject": "stock-value", "version": "latest", "value": r#"{"qty":0}"#}),
    )
    .await;
    assert_eq!(
        (v["ok"].as_bool(), v["error"].as_str()),
        (Some(false), Some("#: required property \"sku\" is missing"))
    );
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/try",
        &json!({"subject": "orders-value", "version": "1", "value": r#"{"id":1}"#}),
    )
    .await;
    assert_eq!(
        (v["ok"].as_bool(), v["size"].as_u64()),
        (Some(true), Some(7)),
        "{v}"
    );
    assert!(
        v["hex"].as_str().unwrap().starts_with("00 00 00 00 01"),
        "{v}"
    );

    let (_, v) = get(
        &app,
        "/api/v1/topics/orders/produce?value=%7B%22id%22%3A5%7D&schema=orders-value",
    )
    .await;
    assert_eq!(v["suggested"], "orders-value");
    assert_eq!(
        (v["encoded"]["ok"].as_bool(), v["encoded"]["kind"].as_str()),
        (Some(true), Some("AVRO")),
        "{v}"
    );
    let (_, v) = post(
        &app,
        "/api/v1/topics/orders/produce",
        &[
            ("value", r#"{"id":"x"}"#),
            ("schema", "orders-value"),
            ("partition", "0"),
        ],
    )
    .await;
    assert_eq!(v["outcome"], "invalid");
    assert_eq!(
        v["message"],
        "the value does not fit orders-value version 1: id: expected a number, not \"x\""
    );
    let (_, v) = post(
        &app,
        "/api/v1/topics/orders/produce",
        &[
            ("value", r#"{"id":5}"#),
            ("schema", "orders-value"),
            ("partition", "0"),
        ],
    )
    .await;
    assert_eq!(v["outcome"], "done", "{v}");
    let (_, v) = get(&app, "/api/v1/topics/orders/records/0/5").await;
    assert_eq!(
        v["value"]["schema"]["json"],
        "{\n  \"id\": 5,\n  \"note\": null\n}"
    );

    let (_, v) = get(&app, "/api/v1/topics/orders/schemas").await;
    let subjects: Vec<(&str, &str)> = v["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["subject"].as_str().unwrap(),
                x["position"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        subjects,
        [
            ("orders-value", "value"),
            ("pay-value", "carried"),
            ("stock-value", "carried")
        ]
    );
    assert_eq!(v["unknown"], json!([99]));
    w.stop().await;
}
