//! The topic page's Empty (CON-1590, CON-1591) over a real broker in this process: the preview says what goes per partition
//! and which groups fall below the new start; one DeleteRecords takes every partition to its high watermark and the topic
//! carries on with its id and configs; the groups asked for are moved to the new start (one with members is refused, in
//! Kafka's words); the name must be typed; a topic that only compacts is refused as Kafka refuses it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_api;
mod support_groups;
mod support_log;
mod support_write;

use axum::http::StatusCode;
use serde_json::Value;
use support_api::{api, get, post};
use support_log::records;
use support_write::world;

#[tokio::test]
async fn emptying_takes_every_partition_to_its_end_in_one_call_and_moves_the_groups_asked_for() {
    let w = world("empty-topic", |_, _| {}).await;
    w.create_topic("orders", 3, &[("retention.ms", "86400000")])
        .await;
    for (p, n) in [(0, 40), (1, 25), (2, 0)] {
        if n > 0 {
            assert_eq!(w.produce_batch("orders", p, records(n), None).await.0, 0);
        }
    }
    support_groups::commit(&w, "fulfilment", "orders", &[(0, 10), (1, 25)]).await;
    support_groups::commit(&w, "audit", "orders", &[(0, 40)]).await;
    // Committed while empty, then joined: a group with a member now.
    support_groups::commit(&w, "busy", "orders", &[(0, 5)]).await;
    support_groups::stable_group(&w, "busy", "range", &[("orders", &[0])]).await;
    let app = api(&w);
    let (s, f) = get(&app, "/api/v1/topics/orders/empty").await;
    assert_eq!(s, StatusCode::OK, "{f}");
    let v = &f["view"];
    assert_eq!(v["records"], 65);
    let parts: Vec<(i64, i64, i64)> = v["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["partition"].as_i64().unwrap(),
                p["high_watermark"].as_i64().unwrap(),
                p["records"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(parts, [(0, 40, 40), (1, 25, 25), (2, 0, 0)]);
    // Below the new start: fulfilment on partition 0 (30 unread), busy on 0; audit is at the end, and partition 1 too.
    let groups: Vec<(String, i64)> = v["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["group"].as_str().unwrap().to_owned(),
                g["unread"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        groups,
        [("busy".to_owned(), 35), ("fulfilment".to_owned(), 30)]
    );
    assert_eq!(v["compact_only"], false);
    assert_eq!(f["refusal"], Value::Null);
    assert_eq!(f["button"], "Empty orders");
    let words = f["summary"].to_string();
    assert!(
        words.contains("65 records") && words.contains("2 partitions"),
        "{words}"
    );
    assert!(
        f["groups_warning"]
            .to_string()
            .contains("OFFSET_OUT_OF_RANGE")
    );
    assert!(f["view"]["call"].to_string().contains("offset: -1"));
    // The name, typed.
    let (s, bad) = post(&app, "/api/v1/topics/orders/empty", &[("confirm", "order")]).await;
    assert_eq!(
        (s, bad["outcome"].as_str()),
        (StatusCode::OK, Some("invalid")),
        "{bad}"
    );
    assert_eq!(bad["message"], "Type orders to confirm.");
    assert_eq!(
        get(&app, "/api/v1/topics/orders/empty").await.1["view"]["records"],
        65,
        "nothing went"
    );
    // Done, with two groups asked to move: the empty one moves, the one with a member is refused in Kafka's words.
    let (s, done) = post(
        &app,
        "/api/v1/topics/orders/empty",
        &[
            ("confirm", "orders"),
            ("group", "fulfilment"),
            ("group", "busy"),
            ("group", "audit"),
        ],
    )
    .await;
    assert_eq!(
        (s, done["outcome"].as_str()),
        (StatusCode::OK, Some("done")),
        "{done}"
    );
    let e = &done["emptied"];
    assert_eq!(e["records"], 65);
    assert_eq!(e["starts"], serde_json::json!([[0, 40], [1, 25], [2, 0]]));
    let moved = e["moved"].as_array().unwrap();
    assert_eq!(
        moved.len(),
        2,
        "audit was not below: not asked of Kafka: {moved:?}"
    );
    let by = |g: &str| moved.iter().find(|m| m["group"] == g).unwrap().clone();
    assert!(
        by("fulfilment")["moved"].as_u64().unwrap() >= 1,
        "{moved:?}"
    );
    assert_eq!(by("fulfilment")["refused"], Value::Null);
    assert_eq!(by("busy")["moved"], 0);
    assert!(
        by("busy")["refused"]
            .as_str()
            .unwrap()
            .contains("UNKNOWN_MEMBER_ID"),
        "{moved:?}"
    );
    // The topic carries on: the same partitions and configs, nothing left to read, the next offsets where they ended.
    let (_, after) = get(&app, "/api/v1/topics/orders/empty").await;
    assert_eq!(after["view"]["records"], 0);
    assert_eq!(after["button"], "Nothing to empty");
    assert_eq!(
        w.config("orders", "retention.ms").await.as_deref(),
        Some("86400000")
    );
    assert_eq!(w.offset("orders", 0, -2).await, 40);
    // fulfilment was moved to the new start: nothing is below it now (busy was refused and still is).
    let groups: Vec<String> = get(&app, "/api/v1/topics/orders/empty").await.1["view"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["group"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(groups, ["busy"]);
    assert_eq!(
        w.produce_batch("orders", 0, records(1), None).await,
        (0, 40)
    );
    w.stop().await;
}

#[tokio::test]
async fn a_topic_that_only_compacts_is_refused_before_the_button_and_by_the_broker() {
    let w = world("empty-compact", |_, _| {}).await;
    w.create_topic("levels", 1, &[("cleanup.policy", "compact")])
        .await;
    w.fill("levels", 5, 2).await;
    let app = api(&w);
    let (_, f) = get(&app, "/api/v1/topics/levels/empty").await;
    assert_eq!(f["view"]["compact_only"], true);
    assert_eq!(f["refusal"]["name"], "POLICY_VIOLATION");
    assert!(f["refused"].as_str().unwrap().contains("Data page"), "{f}");
    let (s, r) = post(
        &app,
        "/api/v1/topics/levels/empty",
        &[("confirm", "levels")],
    )
    .await;
    assert_eq!(
        (s, r["outcome"].as_str(), r["code"].as_str()),
        (StatusCode::OK, Some("refused"), Some("POLICY_VIOLATION")),
        "{r}"
    );
    assert!(
        r["detail"]["refused"]
            .as_str()
            .unwrap()
            .contains("nothing was removed"),
        "{r}"
    );
    assert_eq!(
        get(&app, "/api/v1/topics/levels/empty").await.1["view"]["records"],
        5,
        "nothing went"
    );
    let (_, none) = get(&app, "/api/v1/topics/no-such/empty").await;
    assert!(none["missing"].is_object(), "{none}");
    w.stop().await;
}
