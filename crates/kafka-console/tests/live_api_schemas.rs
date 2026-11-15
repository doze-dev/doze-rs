//! Slice 10d: the Schemas pages' JSON API over a real broker and a registry in the same process: no registry, then register,
//! check, list, a subject, diff, levels and deletes.
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
use axum::http::StatusCode;
use serde_json::json;
use support_api::{api, get};
use support_schemas::{V1, V2, V3_BAD, draft, post_json, with_registry};
use support_write::world;

/// CON-1500: with no registry the list says why and every other Schemas call is SCHEMAS_OFF; the sidebar has no count;
/// Connect has no registry lines.
#[tokio::test]
async fn without_a_registry_the_pages_say_why() {
    let w = world("schemas-off", |_, _| {}).await;
    let app = api(&w);
    let (s, v) = get(&app, "/api/v1/schemas").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["off"]["title"], "Not in this build");
    assert!(
        v["off"]["command"]
            .as_str()
            .unwrap()
            .ends_with("--schema-registry"),
        "{v}"
    );
    let (s, v) = get(&app, "/api/v1/schemas/subjects/orders-value").await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::CONFLICT, Some("SCHEMAS_OFF"))
    );
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
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/check",
        &draft("orders-value", "AVRO", V1),
    )
    .await;
    assert_eq!(v["outcome"], "first", "{v}");
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/register",
        &draft("orders-value", "AVRO", V1),
    )
    .await;
    assert_eq!(
        (
            v["outcome"].as_str(),
            v["version"].as_u64(),
            v["existed"].as_bool()
        ),
        (Some("registered"), Some(1), Some(false)),
        "{v}"
    );
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/register",
        &draft("orders-value", "AVRO", V1),
    )
    .await;
    assert_eq!(v["existed"], true, "the same schema again creates nothing");
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/check",
        &draft("orders-value", "AVRO", V2),
    )
    .await;
    assert_eq!(
        (v["outcome"].as_str(), v["level"].as_str()),
        (Some("compatible"), Some("BACKWARD")),
        "{v}"
    );
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/check",
        &draft("orders-value", "AVRO", V3_BAD),
    )
    .await;
    assert_eq!(v["outcome"], "incompatible", "{v}");
    let reason = &v["findings"][0]["reasons"][0];
    assert_eq!(reason["kind"], "READER_FIELD_MISSING_DEFAULT_VALUE");
    assert_eq!(
        reason["description"],
        "The field 'qty' at path '/fields/1' in the new schema has no default value and is missing in the old schema"
    );
    assert_eq!(reason["fix"]["label"], "Add \"default\": 0 to qty");
    let fixed = reason["fix"]["schema"].as_str().unwrap().to_owned();
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/check",
        &draft("orders-value", "AVRO", &fixed),
    )
    .await;
    assert_eq!(v["outcome"], "compatible", "the fix makes it pass: {v}");
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/register",
        &draft("orders-value", "AVRO", V3_BAD),
    )
    .await;
    assert_eq!(
        (
            v["outcome"].as_str(),
            v["status"].as_u64(),
            v["error_code"].as_u64()
        ),
        (Some("refused"), Some(409), Some(409)),
        "{v}"
    );
    assert_eq!(
        v["findings"][0]["reasons"][0]["kind"],
        "READER_FIELD_MISSING_DEFAULT_VALUE"
    );
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/check",
        &draft("orders-value", "AVRO", "{nope"),
    )
    .await;
    assert_eq!(
        (v["outcome"].as_str(), v["error_code"].as_u64()),
        (Some("invalid"), Some(42201)),
        "{v}"
    );
    post_json(
        &app,
        "/api/v1/schemas/register",
        &draft("orders-value", "AVRO", V2),
    )
    .await;

    let (_, v) = get(&app, "/api/v1/schemas").await;
    assert!(v["off"].is_null());
    let row = &v["subjects"][0];
    assert_eq!(
        (
            row["subject"].as_str(),
            row["kind"].as_str(),
            row["versions"].as_u64()
        ),
        (Some("orders-value"), Some("AVRO"), Some(2))
    );
    assert_eq!(row["topics"], json!([["orders", "value"]]));
    assert_eq!(get(&app, "/api/v1/shell").await.1["schemas"], 1);
    assert_eq!(
        get(&app, "/api/v1/topics").await.1["subjects"],
        json!([["orders", "orders-value"]])
    );
    assert_eq!(
        get(&app, "/api/v1/connect").await.1["registry"]["properties"],
        "schema.registry.url=http://127.0.0.1:18081"
    );

    let (_, v) = get(&app, "/api/v1/schemas/subjects/orders-value").await;
    assert_eq!(v["shown"]["version"], 2);
    assert_eq!(
        v["shown"]["schema"],
        "{\n  \"type\": \"record\",\n  \"name\": \"Order\",\n  \"namespace\": \"shop\",\n  \"fields\": [\n    {\"name\": \"id\", \"type\": \"long\"},\n    {\"name\": \"note\", \"type\": [\"null\", \"string\"], \"default\": null}\n  ]\n}"
    );
    assert_eq!(v["versions"][0]["registered_by"], "this console");
    assert_eq!(v["shown"]["sample"], "{\n  \"id\": 0,\n  \"note\": null\n}");
    let (_, v) = get(&app, "/api/v1/schemas/subjects/orders-value?version=1").await;
    assert_eq!(v["shown"]["version"], 1);
    let (_, v) = get(&app, "/api/v1/schemas/subjects/nope").await;
    assert_eq!(v["missing"]["code"], "SUBJECT_NOT_FOUND");

    let (_, v) = get(&app, "/api/v1/schemas/subjects/orders-value/diff").await;
    // One field a line: the new field is one added line, and the line before it gains a comma.
    assert_eq!(
        (
            v["from"].as_u64(),
            v["to"].as_u64(),
            v["removed"].as_u64(),
            v["added"].as_u64()
        ),
        (Some(1), Some(2), Some(1), Some(2)),
        "{v}"
    );
    let checks = v["checks"].as_array().unwrap();
    assert!(
        checks.iter().all(|c| c["ok"] == true),
        "a nullable field with a default passes all three: {v}"
    );

    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/subjects/orders-value/config",
        &json!({"level": "NONE"}),
    )
    .await;
    assert_eq!(
        (v["outcome"].as_str(), v["level"].as_str()),
        (Some("configured"), Some("NONE"))
    );
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/subjects/orders-value/config",
        &json!({"level": "default", "mode": "READONLY"}),
    )
    .await;
    assert_eq!(
        (v["level"].as_str(), v["mode"].as_str()),
        (Some("BACKWARD"), Some("READONLY"))
    );
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/subjects/orders-value/delete",
        &json!({"version": 1}),
    )
    .await;
    assert_eq!(
        (v["outcome"].as_str(), v["error_code"].as_u64()),
        (Some("refused"), Some(42205)),
        "read-only refuses: {v}"
    );
    post_json(
        &app,
        "/api/v1/schemas/subjects/orders-value/config",
        &json!({"mode": "READWRITE"}),
    )
    .await;
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/subjects/orders-value/delete",
        &json!({"version": 1, "permanent": true}),
    )
    .await;
    assert_eq!(
        v["error_code"], 40407,
        "permanent before soft is refused in Confluent's code: {v}"
    );
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/subjects/orders-value/delete",
        &json!({"version": 1}),
    )
    .await;
    assert_eq!(
        (v["outcome"].as_str(), v["versions"].clone()),
        (Some("deleted"), json!([1]))
    );
    let (s, v) = post_json(
        &app,
        "/api/v1/schemas/subjects/orders-value/delete",
        &json!({"permanent": true}),
    )
    .await;
    assert_eq!(
        (s, v["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("CONFIRM"))
    );
    post_json(
        &app,
        "/api/v1/schemas/subjects/orders-value/delete",
        &json!({}),
    )
    .await;
    let (_, v) = get(&app, "/api/v1/schemas").await;
    assert_eq!(
        v["subjects"][0]["deleted"], true,
        "a soft-deleted subject is listed as such"
    );
    let (_, v) = post_json(
        &app,
        "/api/v1/schemas/subjects/orders-value/delete",
        &json!({"permanent": true, "confirm": "orders-value"}),
    )
    .await;
    assert_eq!(v["outcome"], "deleted", "{v}");
    assert_eq!(get(&app, "/api/v1/schemas").await.1["subjects"], json!([]));
    w.stop().await;
}
