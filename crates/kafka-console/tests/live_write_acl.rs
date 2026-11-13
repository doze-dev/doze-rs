//! A GET never writes: every read of the API, over a real broker through a backend that records what it was asked.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_write;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use kafka_wire::messages::ApiKey;
use support::api_get;
use support_write::{World, page, post_form, router_of, world};

/// A write as the React app sends it: the form fields, the proof of origin and the console's header.
fn api_post(path: &str, fields: &[(&str, &str)]) -> Request<Body> {
    let mut req = post_form(path, fields);
    req.headers_mut().insert("x-console", "1".parse().unwrap());
    req
}

async fn produce_many(w: &World, topic: &str, n: usize) {
    let app = w.router();
    for i in 0..n {
        let value = format!("v{i}");
        let (status, doc) = page(
            &app,
            api_post(
                &format!("/api/v1/topics/{topic}/produce"),
                &[
                    ("partition", "0"),
                    ("key", "k"),
                    ("value", &value),
                    ("acks", "1"),
                ],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(doc.contains("\"outcome\":\"done\""), "{doc:.300}");
    }
}

#[tokio::test]
async fn a_get_never_asks_the_broker_to_change_anything() {
    let w = world("spy", |_, _| {}).await;
    w.create_topic("orders", 3, &[]).await;
    produce_many(&w, "orders", 2).await;
    let (spy, log) = w.backend.recording();
    let app = router_of(&w, spy);
    let before = (
        w.topics().await,
        w.offset("orders", 0, -1).await,
        w.offset("orders", 0, -2).await,
    );
    let mut asked = 0;
    let more = [
        "/api/v1/topics/orders/messages?partition=0",
        "/api/v1/topics/orders/records/0/1",
        "/api/v1/topics/orders/log?partition=0",
        "/api/v1/topics/orders/producers?partition=0",
        "/api/v1/groups/g",
        "/api/v1/groups/g/story",
        "/api/v1/groups/g/reset?topic=orders&mode=earliest",
        "/api/v1/groups/g/delete",
        "/api/v1/groups/g/offsets/delete",
        "/api/v1/settings/edit?key=log.retention.ms",
        "/api/v1/access/acls/new",
        "/api/v1/lab/once/drop",
        "/api/v1/lab/compact?topic=orders",
    ];
    for path in kafka_console::api::GET_PATHS.iter().chain(&more) {
        let (status, body) = page(&app, api_get(path)).await;
        assert!(
            status.as_u16() < 500,
            "{path} answered {status}: {body:.300}"
        );
        asked += 1;
    }
    assert!(asked > 30, "{asked}");
    let reads = [
        ApiKey::Metadata,
        ApiKey::ListOffsets,
        ApiKey::DescribeConfigs,
        ApiKey::ListGroups,
        ApiKey::DescribeGroups,
        ApiKey::ConsumerGroupDescribe,
        ApiKey::OffsetFetch,
        ApiKey::ListTransactions,
        ApiKey::DescribeTransactions,
        ApiKey::ApiVersions,
        ApiKey::Fetch,
        ApiKey::DescribeProducers,
        // Slice 9h: the Access page reads the ACLs and the names of the users.
        ApiKey::DescribeAcls,
        ApiKey::DescribeUserScramCredentials,
    ];
    let calls = log.lock().unwrap().clone();
    assert!(
        !calls.is_empty(),
        "the API asked the broker for what they show"
    );
    for call in &calls {
        assert!(
            reads.contains(call),
            "a GET called {call:?}, which is not a read"
        );
    }
    assert_eq!(
        (
            w.topics().await,
            w.offset("orders", 0, -1).await,
            w.offset("orders", 0, -2).await
        ),
        before,
        "and nothing changed"
    );
    // The same pages, then a POST: the spy sees the write.
    let (status, _) = page(
        &app,
        api_post(
            "/api/v1/topics/orders/produce",
            &[("value", "x"), ("acks", "1")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(log.lock().unwrap().contains(&ApiKey::Produce));
    w.stop().await;
}
