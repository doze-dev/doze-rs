//! The Groups API of the React console (slice R2) over the fixtures: what each page reads, and the three writes with Kafka's
//! refusal in its own name and words, the console's reading of it, and the proof a write needs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use kafka_console::app::console;
use kafka_console::base::Base;
use kafka_console::limits::Limits;
use kafka_console::source::Source;
use kafka_console::state::State;
use serde_json::Value;
use support::{HOST, send};
use tokio_util::sync::CancellationToken;

fn app() -> Router {
    console(State::console(
        Source::Fixture,
        &Base::new("/"),
        vec![],
        CancellationToken::new(),
        &Limits::default(),
    ))
}

async fn get(app: &Router, path: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .unwrap();
    let (res, body) = send(app, req).await;
    (res.status(), serde_json::from_str(&body).unwrap())
}

async fn post(app: &Router, path: &str, body: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    let (res, text) = send(app, req).await;
    (res.status(), serde_json::from_str(&text).unwrap())
}

#[tokio::test]
async fn the_groups_api_says_what_the_list_a_group_and_its_story_show() {
    let app = app();
    let (s, list) = get(&app, "/api/v1/groups").await;
    assert_eq!(s, StatusCode::OK);
    let rows = list["view"]["rows"].as_array().unwrap();
    assert!(rows.len() >= 3, "{list}");
    assert_eq!(list["view"]["counts"].as_array().unwrap().len(), 4);
    assert_eq!(list["states"], serde_json::json!([]));
    // The states are a repeated field, kept in the list's order; the rows are those in ANY of them.
    let (_, some) = get(
        &app,
        "/api/v1/groups?state=empty&state=stable&state=nonsense",
    )
    .await;
    assert_eq!(some["states"], serde_json::json!(["stable", "empty"]));
    assert!(
        some["view"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["phase"] == "stable" || r["phase"] == "empty"),
        "{some}"
    );
    let (_, named) = get(&app, "/api/v1/groups?q=pay").await;
    assert!(
        named["view"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["id"].as_str().unwrap().contains("pay"))
    );
    // A rebalancing group is said in Rust's words, as spans.
    let (_, all) = get(&app, "/api/v1/groups").await;
    if all["view"]["attention"].is_object() {
        assert_eq!(all["attention"][0]["kind"], "strong");
    }
    // One group: its head, its lag per partition with the lag as a tagged value.
    let (s, g) = get(&app, "/api/v1/groups/search-indexer").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(g["head"]["id"], "search-indexer");
    assert!(g["head"]["groups"].as_array().unwrap().len() >= 3);
    let lag = &g["topics"][0]["rows"][0]["lag"];
    assert!(lag["kind"].is_string(), "{g}");
    assert!(!g.to_string().contains("resident"));
    let (_, expiring) = get(&app, "/api/v1/groups/analytics?expiring=1").await;
    assert_eq!(expiring["state"], "expiring");
    // The story: generations newest first, the flapping rule worked out, causes as words.
    let (s, story) = get(&app, "/api/v1/groups/fulfilment/story").await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        story["generations"]
            .as_array()
            .is_some_and(|g| !g.is_empty())
    );
    assert!(story["flap"]["rows"].as_array().is_some());
    assert!(story["generations"][0]["cause"].is_string());
    // A group the broker does not know: a JSON 404 that names it.
    let (s, none) = get(&app, "/api/v1/groups/no-such-group").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(none["code"], "GROUP_NOT_FOUND");
    assert!(none["message"].as_str().unwrap().contains("no-such-group"));
}

#[tokio::test]
async fn a_reset_is_previewed_with_a_get_and_refused_in_kafkas_words_while_the_group_has_members() {
    let app = app();
    let (s, preview) = get(
        &app,
        "/api/v1/groups/search-indexer/reset?topic=clickstream&to=earliest",
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(preview["to"], "earliest");
    assert!(!preview["rows"].as_array().unwrap().is_empty());
    assert!(preview["call"].as_array().is_some());
    let (_, bad) = get(
        &app,
        "/api/v1/groups/search-indexer/reset?topic=clickstream&to=offset&value=abc",
    )
    .await;
    assert_eq!(bad["error"], "The offset must be a whole number.");
    // A group with a member: Kafka refuses the commit; the answer is its name, its words, and the console's reading.
    let (s, r) = post(
        &app,
        "/api/v1/groups/search-indexer/reset",
        r#"{"topic":"clickstream","to":"earliest","value":""}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["outcome"], "refused", "a refusal is an answer: {r}");
    assert!(
        r["code"]
            .as_str()
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_')
    );
    assert!(!r["message"].as_str().unwrap().is_empty());
    assert!(r["number"].as_i64().unwrap() > 0);
    assert_eq!(r["refused"]["what"], "Reset offsets");
    assert!(!r["refused"]["steps"].as_array().unwrap().is_empty());
    // An empty group: done, with how many offsets moved.
    let (s, done) = post(
        &app,
        "/api/v1/groups/analytics/reset",
        r#"{"topic":"orders","to":"latest","value":""}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(done["outcome"], "done");
    assert!(done["done"].as_u64().unwrap() > 0);
    // Not JSON: refused before anything is asked of the broker.
    let (s, e) = post(&app, "/api/v1/groups/analytics/reset", "to=latest").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(e["code"], "BAD_REQUEST");
}

#[tokio::test]
async fn deleting_a_group_with_members_is_refused_as_non_empty_group() {
    let app = app();
    let (s, form) = get(&app, "/api/v1/groups/search-indexer/delete").await;
    assert_eq!(s, StatusCode::OK);
    assert!(form["offsets"].as_array().is_some());
    let (s, r) = post(&app, "/api/v1/groups/search-indexer/delete", "{}").await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["outcome"], "refused", "a refusal is an answer: {r}");
    assert_eq!(r["code"], "NON_EMPTY_GROUP");
    assert_eq!(r["number"], 68);
    assert_eq!(r["message"], "The group is not empty.");
    assert_eq!(r["refused"]["what"], "Delete group");
    let (s, done) = post(&app, "/api/v1/groups/analytics/delete", "{}").await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(done["outcome"], "done");
}

#[tokio::test]
async fn deleting_offsets_needs_a_topic_and_is_refused_while_a_member_reads_it() {
    let app = app();
    let (s, form) = get(&app, "/api/v1/groups/analytics/offsets/delete").await;
    assert_eq!(s, StatusCode::OK);
    let topics = form["topics"].as_array().unwrap();
    assert!(!topics.is_empty());
    let (s, none) = post(
        &app,
        "/api/v1/groups/analytics/offsets/delete",
        r#"{"topics":[]}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{none}");
    assert_eq!(none["outcome"], "invalid");
    assert!(none["offsets"].is_object());
    let first = topics[0][0].as_str().unwrap();
    let (s, done) = post(
        &app,
        "/api/v1/groups/analytics/offsets/delete",
        &format!(r#"{{"topics":["{first}"]}}"#),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(done["outcome"], "done");
    let (_, busy) = get(&app, "/api/v1/groups/search-indexer/offsets/delete").await;
    let topic = busy["topics"][0][0].as_str().unwrap();
    let (s, r) = post(
        &app,
        "/api/v1/groups/search-indexer/offsets/delete",
        &format!(r#"{{"topics":["{topic}"]}}"#),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["outcome"], "refused", "a refusal is an answer: {r}");
    assert_eq!(r["code"], "GROUP_SUBSCRIBED_TO_TOPIC");
    assert_eq!(r["number"], 86);
}

#[tokio::test]
async fn every_get_of_one_group_is_json_that_no_cache_keeps_and_carries_no_memory_figure() {
    let app = app();
    for path in kafka_console::api::groups::GROUP_GET_PATHS {
        let req = Request::builder()
            .uri(*path)
            .header(header::HOST, HOST)
            .header("x-console", "1")
            .header("sec-fetch-site", "same-origin")
            .body(Body::empty())
            .unwrap();
        let (res, body) = send(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}: {body}");
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store", "{path}");
        assert!(
            body.starts_with('{') && !body.contains("resident"),
            "{path}"
        );
        // Without the console's header: refused.
        let bare = Request::builder()
            .uri(*path)
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap();
        let (res, _) = send(&app, bare).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn a_group_write_needs_the_same_origin_proof() {
    let app = app();
    for path in [
        "/api/v1/groups/analytics/reset",
        "/api/v1/groups/analytics/delete",
        "/api/v1/groups/analytics/offsets/delete",
    ] {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::HOST, HOST)
            .header("x-console", "1")
            .header(header::ORIGIN, "http://evil.example")
            .body(Body::from("{}"))
            .unwrap();
        let (res, body) = send(&app, req).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}: {body}");
    }
}
