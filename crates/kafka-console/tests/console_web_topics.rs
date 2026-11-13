//! The Topics API of the React console (slice R1a) over the fixtures: the list, a topic's frame, its Messages and one record,
//! Consumers, Log (and Compact now refused without a live Lab), Producers, a transaction, the Messages tail, and the words
//! for a name that is no topic.
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

fn req(method: &str, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .unwrap()
}

async fn get(app: &Router, path: &str) -> (StatusCode, Value) {
    let (res, body) = send(app, req("GET", path)).await;
    let status = res.status();
    (status, serde_json::from_str(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn the_topics_list_and_a_topics_frame_are_the_view_models_serialised() {
    let app = app();
    let (s, list) = get(&app, "/api/v1/topics").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(list["state"], "list");
    let rows = list["view"]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["name"] == "orders"));
    assert!(
        rows.iter().all(|r| r["internal"] == false),
        "internal hidden"
    );
    let (_, with) = get(&app, "/api/v1/topics?internal=1").await;
    assert!(
        with["view"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "__consumer_offsets")
    );
    let (_, none) = get(&app, "/api/v1/topics?q=zzz-no-such").await;
    assert_eq!(none["state"], "filtered");
    let (s, f) = get(&app, "/api/v1/topics/orders").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(f["head"]["name"], "orders");
    assert_eq!(f["acts"], "full");
    assert!(f["retention"].as_str().unwrap().starts_with("delete"));
    assert!(f["records"].as_i64().unwrap() > 0);
    assert!(!f["head"]["parts"].as_array().unwrap().is_empty());
    let (s, missing) = get(&app, "/api/v1/topics/no-such-topic").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(missing["code"], "UNKNOWN_TOPIC_OR_PARTITION");
    assert!(
        missing["message"]
            .as_str()
            .unwrap()
            .contains("no topic called no-such-topic")
    );
    let (s, _) = get(&app, "/api/v1/topics/bad%2Fname").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn messages_take_their_filters_from_the_query_and_a_record_is_whole() {
    let app = app();
    let (s, m) = get(&app, "/api/v1/topics/orders/messages").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(m["tailable"], true);
    let rows = m["view"]["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    assert!(
        m["reach"].as_array().unwrap().is_empty(),
        "no filter, no sentence"
    );
    let (_, f) = get(
        &app,
        "/api/v1/topics/orders/messages?partition=1&isolation=read_committed&key=ord-1",
    )
    .await;
    assert_eq!(f["view"]["query"]["partitions"], serde_json::json!([1]));
    assert_eq!(f["view"]["query"]["reading"], "committed");
    assert_eq!(f["tailable"], false);
    assert!(!f["reach"].as_array().unwrap().is_empty());
    let (_, bad) = get(&app, "/api/v1/topics/orders/messages?offset=abc").await;
    assert!(
        bad["view"]["notes"][0]
            .as_str()
            .unwrap()
            .contains("is not an offset")
    );
    let r0 = &rows[0];
    let path = format!(
        "/api/v1/topics/orders/records/{}/{}",
        r0["partition"], r0["offset"]
    );
    let (s, rec) = get(&app, &path).await;
    assert_eq!(s, StatusCode::OK, "{path}");
    assert_eq!(rec["topic"], "orders");
    assert!(rec["value"]["kind"].is_string());
    let (s, gone) = get(&app, "/api/v1/topics/orders/records/0/999999999").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(gone["code"], "NO_SUCH_RECORD");
}

#[tokio::test]
async fn consumers_log_producers_and_a_transaction_answer_with_their_words() {
    let app = app();
    let (s, c) = get(&app, "/api/v1/topics/orders/consumers").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(c["topic"], "orders");
    assert!(c["readers"].is_array());
    let (s, l) = get(&app, "/api/v1/topics/orders/log").await;
    assert_eq!(s, StatusCode::OK);
    assert!(l["state"].is_string());
    assert!(l["ruler"]["cells"].is_array());
    assert!(
        l["compact"]["off"].as_bool().unwrap(),
        "no Lab over the fixtures"
    );
    let (s, p) = get(&app, "/api/v1/topics/orders/producers").await;
    assert_eq!(s, StatusCode::OK);
    assert!(p["view"]["producers"].is_array());
    let (s, t) = get(&app, "/api/v1/topics/orders/transactions/orders-eos").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(t["view"]["txn"]["id"], "orders-eos");
    assert!(!t["sentence"].as_array().unwrap().is_empty());
    let (s, none) = get(&app, "/api/v1/topics/orders/transactions/nobody").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(none["code"], "TRANSACTIONAL_ID_NOT_FOUND");
}

#[tokio::test]
async fn compact_now_is_a_post_with_the_proof_and_is_refused_without_a_live_lab() {
    let app = app();
    // A GET of the write is not a read.
    let (res, _) = send(&app, req("GET", "/api/v1/topics/orders/log/compact")).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    // Without the same-origin proof: refused before anything runs.
    let bare = Request::builder()
        .method("POST")
        .uri("/api/v1/topics/orders/log/compact")
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .body(Body::empty())
        .unwrap();
    let (res, _) = send(&app, bare).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let (res, body) = send(
        &app,
        req("POST", "/api/v1/topics/orders/log/compact?partition=0"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT, "{body}");
    assert!(body.contains("\"code\":\"LAB_OFF\""), "{body}");
}

#[tokio::test]
async fn the_messages_tail_is_an_event_stream_that_starts_with_ready_and_refuses_a_fixed_read() {
    let app = app();
    let (res, body) = send(
        &app,
        req("GET", "/api/v1/tail/messages?topic=orders&key=ord-1"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("only the newest records can be tailed"));
    let (res, _) = send(&app, req("GET", "/api/v1/tail/messages?topic=no-such")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    // An open stream: its first event says it is open. Read the head of the body only (the stream does not end).
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let res = app
        .oneshot(req("GET", "/api/v1/tail/messages?topic=orders&pace=1"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let mut body = res.into_body();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame())
        .await
        .expect("a first event within 5 s")
        .unwrap()
        .unwrap();
    let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(text.contains("event: ready"), "{text}");
}
