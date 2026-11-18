//! The Data page over a real broker in this process (docs/port/data-control.md): what is using the disk, Empty topic, Empty all
//! topics and Erase everything, each answered `done`, `refused` in Kafka's words, or `invalid` when the phrase was not typed.
#![allow(
    clippy::too_many_lines,
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
use kafka_auth::Principal;
use kafka_broker::Caller;
use support_api::{api, get, post};
use support_groups::{commit, committed, produce};
use support_write::{router_of, world};

#[tokio::test]
async fn the_page_says_each_kind_and_each_topic_largest_first_with_why() {
    let w = world("data-page", |_, _| {}).await;
    w.create_topic("big", 2, &[("retention.ms", "604800000")])
        .await;
    w.create_topic("small", 1, &[]).await;
    w.create_topic("keys", 1, &[("cleanup.policy", "compact")])
        .await;
    produce(&w, "big", 0, 200).await;
    produce(&w, "big", 1, 200).await;
    produce(&w, "small", 0, 3).await;
    commit(&w, "readers", "small", &[(0, 1)]).await;
    let app = api(&w);
    let (s, v) = get(&app, "/api/v1/data").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let view = &v["view"];
    let names: Vec<&str> = view["topics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["big", "small", "keys"]);
    let big = &view["topics"][0];
    assert_eq!(big["retention"], "7 days");
    assert_eq!(big["unbounded"], false);
    assert!(big["why"].as_str().unwrap().starts_with("Retention 7 days"));
    let small = &view["topics"][1];
    assert_eq!(small["unbounded"], true);
    assert_eq!(small["retention"], "none");
    assert_eq!(small["read_by"], serde_json::json!(["readers"]));
    assert_eq!(view["topics"][2]["retention"], "latest per key");
    assert!(view["total"].as_u64().unwrap() > big["bytes"].as_u64().unwrap());
    assert!(
        view["volume"]["total"].as_u64().unwrap() > 0,
        "the volume says its size here"
    );
    let kinds: Vec<&str> = view["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"topics") && kinds.contains(&"metadata"),
        "{kinds:?}"
    );
    assert!(
        view["defaults"]
            .as_str()
            .unwrap()
            .contains("keeps everything")
    );
    assert_eq!(v["words"]["erase_phrase"], "erase everything");
    w.stop().await;
}

#[tokio::test]
async fn empty_topic_says_what_goes_then_frees_it_and_the_topic_carries_on() {
    let w = world("data-empty", |_, _| {}).await;
    w.create_topic("orders", 2, &[]).await;
    produce(&w, "orders", 0, 40).await;
    produce(&w, "orders", 1, 2).await;
    commit(&w, "fulfilment", "orders", &[(0, 10)]).await;
    let app = api(&w);
    let (s, v) = get(&app, "/api/v1/data/empty?topic=orders").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["view"]["records"], 42);
    assert_eq!(v["view"]["blocked"], serde_json::Value::Null);
    assert!(v["button"].as_str().unwrap().starts_with("Empty and free"));
    let (_, missing) = get(&app, "/api/v1/data/empty?topic=nope").await;
    assert_eq!(missing["missing"]["code"], "UNKNOWN_TOPIC_OR_PARTITION");

    let (s, v) = post(&app, "/api/v1/data/empty", &[("topic", "orders")]).await;
    assert_eq!(
        (s, v["outcome"].as_str()),
        (StatusCode::OK, Some("done")),
        "{v}"
    );
    assert_eq!(v["records"], 42);
    assert!(v["freed_bytes"].as_u64().unwrap() > 0);
    assert!(
        v["sentence"]
            .as_str()
            .unwrap()
            .starts_with("Emptied orders: 42 records in 2 partitions")
    );
    assert_eq!(
        w.offset("orders", 0, -2).await,
        40,
        "earliest is the old end"
    );
    assert_eq!(w.offset("orders", 0, -1).await, 40);
    assert_eq!(
        committed(&w, "fulfilment").await,
        vec![("orders".to_owned(), 0, 10)]
    );
    produce(&w, "orders", 0, 1).await;
    assert_eq!(
        w.offset("orders", 0, -1).await,
        41,
        "the next record took the next offset"
    );
    w.stop().await;
}

#[tokio::test]
async fn the_all_and_everything_actions_ask_for_their_phrase_and_then_do_it() {
    let w = world("data-all", |_, _| {}).await;
    w.create_topic("a", 1, &[]).await;
    w.create_topic("b", 1, &[("cleanup.policy", "compact")])
        .await;
    produce(&w, "a", 0, 5).await;
    produce(&w, "b", 0, 5).await;
    let app = api(&w);
    let (s, v) = post(&app, "/api/v1/data/empty-all", &[("confirm", "empty all")]).await;
    assert_eq!(
        (s, v["outcome"].as_str()),
        (StatusCode::OK, Some("invalid")),
        "{v}"
    );
    assert_eq!(v["message"], "Type empty all topics to confirm.");
    assert_eq!(w.offset("a", 0, -2).await, 0, "nothing was touched");
    let (_, v) = post(
        &app,
        "/api/v1/data/empty-all",
        &[("confirm", "empty all topics")],
    )
    .await;
    assert_eq!(v["outcome"], "done", "{v}");
    assert_eq!(
        (v["topics"].as_u64(), v["records"].as_i64()),
        (Some(2), Some(10))
    );
    assert_eq!(w.offset("b", 0, -2).await, 5, "the compacted topic too");

    let (_, v) = post(&app, "/api/v1/data/erase", &[("confirm", "erase")]).await;
    assert_eq!(v["outcome"], "invalid", "{v}");
    assert_eq!(w.topics().await.len(), 2);
    let (_, v) = post(
        &app,
        "/api/v1/data/erase",
        &[("confirm", "erase everything")],
    )
    .await;
    assert_eq!(v["outcome"], "done", "{v}");
    assert_eq!(v["topics"], 2);
    assert_eq!(
        v["subjects"],
        serde_json::Value::Null,
        "this console has no registry"
    );
    assert!(w.topics().await.is_empty());
    let (_, v) = get(&app, "/api/v1/data").await;
    assert!(v["view"]["topics"].as_array().unwrap().is_empty());
    w.stop().await;
}

/// The console is a super user of every broker; a backend acting as someone else meets the ACLs a client would (DELETE on the
/// topic for Empty topic, ALTER on the cluster for the others), and is told so in Kafka's words.
#[tokio::test]
async fn a_caller_without_the_rights_is_refused_in_kafkas_words_and_nothing_changes() {
    let w = world("data-acl", |b, _| b.acl_default_allow = false).await;
    w.create_topic("orders", 1, &[]).await;
    produce(&w, "orders", 0, 4).await;
    let bob = Caller::new(Principal::user("bob"), Some("127.0.0.1".parse().unwrap()));
    let app = router_of(&w, w.backend.acting_as(bob));
    let (_, v) = post(&app, "/api/v1/data/empty", &[("topic", "orders")]).await;
    assert_eq!(v["outcome"], "refused", "{v}");
    assert_eq!(v["code"], "TOPIC_AUTHORIZATION_FAILED");
    assert!(
        v["message"]
            .as_str()
            .unwrap()
            .contains("DELETE on topic orders")
    );
    let (_, v) = post(
        &app,
        "/api/v1/data/erase",
        &[("confirm", "erase everything")],
    )
    .await;
    assert_eq!(v["code"], "CLUSTER_AUTHORIZATION_FAILED", "{v}");
    let (_, v) = get(&app, "/api/v1/data").await;
    assert_eq!(v["missing"]["code"], "CLUSTER_AUTHORIZATION_FAILED");
    assert_eq!(w.offset("orders", 0, -2).await, 0, "nothing was touched");
    assert_eq!(w.topics().await.len(), 1);
    w.stop().await;
}
