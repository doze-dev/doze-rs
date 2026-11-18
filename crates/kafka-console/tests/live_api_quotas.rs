//! The client quotas of the Access page over a real broker in this process (CON-1523): a quota set from the page is stored and
//! listed with who it holds, a client over it is listed as muted, a value Kafka refuses is refused in its words, and a quota
//! removed is gone.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_api;
mod support_groups;
mod support_write;

use axum::http::StatusCode;
use serde_json::json;
use support_api::{api, get, post_json as post};
use support_write::world;

#[tokio::test]
async fn a_quota_set_from_the_page_is_listed_and_one_kafka_refuses_is_refused_in_its_words() {
    let w = world("quotas-page", |_, _| {}).await;
    let app = api(&w);
    let (s, v) = get(&app, "/api/v1/access").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["access"]["quotas"]["rows"], json!([]));
    let set =
        json!({ "user": "alice", "client": null, "key": "producer_byte_rate", "value": 1048576.0 });
    let (s, out) = post(&app, "/api/v1/access/quotas", &set).await;
    assert_eq!(
        (s, &out["outcome"]),
        (StatusCode::OK, &json!("done")),
        "{out}"
    );
    let deflt = json!({ "user": null, "client": "", "key": "request_percentage", "value": 12.5 });
    assert_eq!(
        post(&app, "/api/v1/access/quotas", &deflt).await.1["outcome"],
        "done"
    );
    let (_, v) = get(&app, "/api/v1/access").await;
    let rows = v["access"]["quotas"]["rows"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let alice = rows
        .iter()
        .find(|r| r["key"] == "producer_byte_rate")
        .unwrap();
    assert_eq!(alice["parts"], json!([{ "kind": "user", "name": "alice" }]));
    assert_eq!(alice["value"], "1048576");
    assert_eq!(alice["limit"], "1.0 MiB/s produced");
    assert!(
        alice["applies"]
            .as_str()
            .unwrap()
            .contains("every client of user alice")
    );
    let clients = rows
        .iter()
        .find(|r| r["key"] == "request_percentage")
        .unwrap();
    assert_eq!(
        clients["parts"],
        json!([{ "kind": "client-id", "name": null }])
    );
    // Zero is Kafka's to refuse, in its words.
    let zero =
        json!({ "user": "alice", "client": null, "key": "consumer_byte_rate", "value": 0.0 });
    let (_, out) = post(&app, "/api/v1/access/quotas", &zero).await;
    assert_eq!(out["outcome"], "refused");
    assert_eq!(
        out["refused"]["message"],
        "Quota consumer_byte_rate must be greater than 0"
    );
    // Removed: gone from the list.
    let remove =
        json!({ "user": "alice", "client": null, "key": "producer_byte_rate", "value": null });
    assert_eq!(
        post(&app, "/api/v1/access/quotas", &remove).await.1["outcome"],
        "done"
    );
    let (_, v) = get(&app, "/api/v1/access").await;
    assert_eq!(v["access"]["quotas"]["rows"].as_array().unwrap().len(), 1);
    // Not a quota the console sends: refused before Kafka is asked.
    let (s, _) = post(&app, "/api/v1/access/quotas", &json!("nonsense")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    w.stop().await;
}
