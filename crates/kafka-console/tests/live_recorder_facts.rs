//! What the topic pages learn from the request recorder (over a real broker, with real requests through it): "written by" on
//! the topics list and a topic's page (debt J23, J160), and a producer's epoch history on the Producers tab (J236).
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_api;
mod support_live;
mod support_log;
mod support_write;

use kafka_wire::messages::*;
use support_live::*;

/// J23, J160: "written by" on the topics list and a topic's page is the clients the tape saw produce to it, newest first;
/// a refused Produce wrote nothing, so its client is not a writer.
#[tokio::test]
async fn the_topics_list_and_a_topic_say_who_the_tape_saw_write_to_them() {
    let (w, rec) = recorded("written-by", false).await;
    w.create_topic("orders", 1, &[]).await;
    w.create_topic("inventory.levels", 1, &[("cleanup.policy", "compact")])
        .await;
    let mut c = Wire::connect(&w).await;
    let keyed = || batch(Some("k"), b"v".to_vec(), None);
    c.call(
        ApiKey::Produce,
        9,
        "orders-svc",
        &produce("orders", keyed(), None),
    )
    .await;
    let tick = || tokio::time::sleep(std::time::Duration::from_millis(5));
    tick().await;
    let mut other = Wire::connect(&w).await;
    other
        .call(
            ApiKey::Produce,
            9,
            "feeder",
            &produce("orders", keyed(), None),
        )
        .await;
    tick().await;
    c.call(
        ApiKey::Produce,
        9,
        "orders-svc",
        &produce("orders", keyed(), None),
    )
    .await;
    // Keyless to a compacted topic: refused INVALID_RECORD.
    let mut third = Wire::connect(&w).await;
    third
        .call(
            ApiKey::Produce,
            9,
            "careless",
            &produce("inventory.levels", batch(None, b"v".to_vec(), None), None),
        )
        .await;
    let app = router(&w, &rec);
    let (s, list) = support_api::get(&app, "/api/v1/topics").await;
    assert_eq!(s, axum::http::StatusCode::OK);
    let row = |name: &str| {
        list["view"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(
        row("orders")["written_by"],
        serde_json::json!(["orders-svc", "feeder"])
    );
    assert_eq!(row("inventory.levels")["written_by"], serde_json::json!([]));
    let (_, frame) = support_api::get(&app, "/api/v1/topics/orders").await;
    assert_eq!(
        frame["written_by"],
        serde_json::json!(["orders-svc", "feeder"])
    );
    w.stop().await;
}

/// J236: the producer's epochs are the InitProducerId answers the tape holds for its id, oldest first, each with when and
/// which client; the Producers answer carries them beside what DescribeProducers says.
#[tokio::test]
async fn a_producer_says_the_epochs_the_tape_saw_it_given() {
    let (w, rec) = recorded("epochs", false).await;
    w.create_topic("orders", 1, &[]).await;
    // The first InitProducerId is not on the tape (asked in process); the two after it are, each raising the epoch.
    let (pid, first) = w.init_producer(Some("orders-eos")).await;
    let mut c = Wire::connect(&w).await;
    c.call(ApiKey::InitProducerId, 4, "orders", &init("orders-eos"))
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    c.call(ApiKey::InitProducerId, 4, "orders", &init("orders-eos"))
        .await;
    let now = (pid, first + 2);
    w.add_to_txn("orders-eos", now, "orders", 0).await;
    assert_eq!(
        w.produce_batch(
            "orders",
            0,
            support_log::from(now.0, now.1, 0, true, 1),
            Some("orders-eos")
        )
        .await
        .0,
        0
    );
    let app = router(&w, &rec);
    let (_, v) = support_api::get(&app, "/api/v1/topics/orders/producers?partition=0").await;
    let p = v["view"]["producers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == pid)
        .unwrap_or_else(|| panic!("{v}"))
        .clone();
    assert_eq!(p["epoch"], now.1);
    let epochs: Vec<(i64, String)> = p["epochs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["epoch"].as_i64().unwrap(),
                e["client"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        epochs,
        [
            (i64::from(first + 1), "orders".to_owned()),
            (i64::from(first + 2), "orders".to_owned())
        ]
    );
    let at: Vec<i64> = p["epochs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["at_ms"].as_i64().unwrap())
        .collect();
    assert!(at[0] < at[1], "{at:?}");
    // J237: the groups whose offsets the transaction staged, which DescribeTransactions does not say, are the AddOffsetsToTxn
    // the tape saw for it (a refused one stages nothing).
    let (_, t) = support_api::get(&app, "/api/v1/topics/orders/transactions/orders-eos").await;
    assert_eq!(t["staged"], serde_json::json!([]), "{t}");
    let mut add = AddOffsetsToTxnRequest::default();
    add.transactional_id = TransactionalId(s("orders-eos"));
    add.producer_id = ProducerId(now.0);
    add.producer_epoch = now.1;
    add.group_id = GroupId(s("fulfilment"));
    c.call(ApiKey::AddOffsetsToTxn, 3, "orders", &add).await;
    let mut stale = add.clone();
    stale.producer_epoch = first;
    stale.group_id = GroupId(s("never-staged"));
    c.call(ApiKey::AddOffsetsToTxn, 3, "orders", &stale).await;
    let (_, t) = support_api::get(&app, "/api/v1/topics/orders/transactions/orders-eos").await;
    assert_eq!(t["staged"], serde_json::json!(["fulfilment"]), "{t}");
    w.stop().await;
}
