//! The Data page's API over the larder: the shapes the page draws, and the refusals it shows.

use axum::http::StatusCode;

use crate::source::Source;

async fn answer(method: &str, path: &str, body: &str) -> (StatusCode, serde_json::Value) {
    use axum::body::Body;
    use axum::http::{Request, header};
    use tower::ServiceExt;
    let state = crate::state::State::new(
        Source::Fixture,
        &crate::base::Base::root(),
        vec![],
        tokio_util::sync::CancellationToken::new(),
    );
    let app = crate::app::console(state);
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1:9093")
        .header(header::ORIGIN, "http://127.0.0.1:9093")
        .header("x-console", "1")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.to_owned()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
async fn the_larder_page_is_largest_first_and_names_the_topic_that_keeps_everything() {
    let (s, v) = answer("GET", "/api/v1/data", "").await;
    assert_eq!(s, StatusCode::OK);
    let topics = v["view"]["topics"].as_array().unwrap();
    assert_eq!(topics[0]["name"], "clickstream");
    let bytes: Vec<u64> = topics
        .iter()
        .map(|t| t["bytes"].as_u64().unwrap())
        .collect();
    assert!(bytes.windows(2).all(|w| w[0] >= w[1]), "{bytes:?}");
    assert_eq!(v["view"]["kinds"][0]["kind"], "topics");
    assert_eq!(v["view"]["strays"][0][0], "audit.trail-0");
    assert_eq!(v["view"]["subjects"], 4);
    assert!(
        v["words"]["erase_keeps"][0]
            .as_str()
            .unwrap()
            .starts_with("ACLs")
    );
}

#[tokio::test]
async fn empty_topic_is_refused_while_a_transaction_is_under_way_and_done_otherwise() {
    let (_, form) = answer("GET", "/api/v1/data/empty?topic=orders", "").await;
    assert!(
        form["view"]["blocked"]
            .as_str()
            .unwrap()
            .contains("payments-tx-1")
    );
    let (s, v) = answer("POST", "/api/v1/data/empty", "topic=orders").await;
    assert_eq!(
        (s, v["outcome"].as_str()),
        (StatusCode::OK, Some("refused"))
    );
    assert_eq!(v["code"], "CONCURRENT_TRANSACTIONS");
    assert_eq!(v["detail"]["code"], 51);
    let (_, v) = answer("POST", "/api/v1/data/empty", "topic=clickstream").await;
    assert_eq!(v["outcome"], "done", "{v}");
    assert!(
        v["sentence"]
            .as_str()
            .unwrap()
            .starts_with("Emptied clickstream")
    );
    let (_, v) = answer("POST", "/api/v1/data/empty", "topic=nope").await;
    assert_eq!(v["missing"]["code"], "UNKNOWN_TOPIC_OR_PARTITION");
}

#[tokio::test]
async fn erase_says_what_it_removed_and_the_group_it_kept() {
    let (_, v) = answer("POST", "/api/v1/data/erase", "confirm=erase+everything").await;
    assert_eq!(v["outcome"], "done", "{v}");
    assert_eq!(v["groups_kept"], serde_json::json!(["search-indexer"]));
    assert!(
        v["sentence"]
            .as_str()
            .unwrap()
            .contains("The group search-indexer has members connected")
    );
}
