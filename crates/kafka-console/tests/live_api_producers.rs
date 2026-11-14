//! The Producers and transaction answers of the Topics API over a real broker in this process (the rules CON-534 to CON-536
//! that drove the server-rendered pages before R5): the producers a partition remembers with their last five batches, the
//! transaction held open that holds the last stable offset, what it holds back and what its timeout will do, and an abort
//! remembered with its marker.
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
mod support_write;

use axum::Router;
use axum::http::StatusCode;
use serde_json::Value;
use support_api::{api, get};
use support_log::{from, seeded};

async fn ok(app: &Router, path: &str) -> Value {
    let (s, v) = get(app, path).await;
    assert_eq!(s, StatusCode::OK, "{path}: {v}");
    v
}

/// The words of a list of spans.
fn said(spans: &Value) -> String {
    spans
        .as_array()
        .unwrap_or_else(|| panic!("not spans: {spans}"))
        .iter()
        .map(|s| s["text"].as_str().unwrap_or(""))
        .collect()
}

/// CON-534: the Producers answer names the producers the partition remembers with their last five batches, and the
/// transaction held open is the one that holds the last stable offset, where the broker says it is.
#[tokio::test]
async fn the_producers_answer_names_the_transaction_that_holds_the_stable_offset() {
    let w = seeded("al-producers").await;
    let app = api(&w);
    let (id, epoch) = w.init_producer(None).await;
    let mut seq = 0;
    for n in [2, 1, 3, 2, 4, 2, 1] {
        assert_eq!(
            w.produce_batch("orders", 0, from(id, epoch, seq, false, n), None)
                .await
                .0,
            0
        );
        seq += i32::try_from(n).unwrap();
    }
    let t = w.init_producer(Some("orders-eos")).await;
    w.add_to_txn("orders-eos", t, "orders", 0).await;
    assert_eq!(
        w.produce_batch("orders", 0, from(t.0, t.1, 0, true, 3), Some("orders-eos"))
            .await
            .0,
        0
    );
    let end = w.offset("orders", 0, -1).await;
    let v = ok(&app, "/api/v1/topics/orders/producers").await;
    let view = &v["view"];
    assert_eq!(view["end"], end);
    assert_eq!(
        view["last_stable"],
        end - 3,
        "the open transaction's first offset"
    );
    assert_eq!(v["held"], 3);
    assert_eq!(view["open"][0]["id"], "orders-eos");
    assert_eq!(view["open"][0]["state"], "Ongoing");
    let producer = |pid: i64| {
        view["producers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == pid)
            .unwrap_or_else(|| panic!("producer {pid}: {view}"))
            .clone()
    };
    let idem = producer(id);
    let batches = idem["batches"].as_array().unwrap();
    assert_eq!(batches.len(), 5, "the last five of seven: {idem}");
    assert_eq!(batches[0]["last_sequence"], seq - 1, "newest first");
    assert_eq!(
        batches[4]["first_sequence"], 3,
        "the two oldest are forgotten"
    );
    assert!(idem["txn"].is_null());
    let txn = producer(t.0);
    assert_eq!(txn["txn"]["id"], "orders-eos");
    assert_eq!(txn["txn"]["from"], end - 3, "{txn}");
    let cells = view["tail"]["cells"].as_array().unwrap();
    assert_eq!(
        cells.iter().filter(|c| *c == "held").count(),
        3,
        "the tail marks the held records: {}",
        view["tail"]
    );
    w.stop().await;
}

/// CON-535: a transaction's answer says what it holds back and what the timeout will do; ended, it holds nothing and is
/// over; a transactional id the broker does not know is a 404.
#[tokio::test]
async fn a_transaction_answer_says_what_it_holds_back_and_what_the_timeout_will_do() {
    let w = seeded("al-txn").await;
    let app = api(&w);
    let t = w.init_producer(Some("orders-eos")).await;
    w.add_to_txn("orders-eos", t, "orders", 1).await;
    assert_eq!(
        w.produce_batch("orders", 1, from(t.0, t.1, 0, true, 2), Some("orders-eos"))
            .await
            .0,
        0
    );
    let v = ok(&app, "/api/v1/topics/orders/transactions/orders-eos").await;
    assert_eq!(v["view"]["txn"]["state"], "Ongoing");
    assert_eq!(v["view"]["txn"]["producer_id"], t.0);
    assert_eq!(v["view"]["txn"]["producer_epoch"], t.1);
    assert_eq!(said(&v["holds"]), "orders-1: 2 records, 0 to 1.", "{v}");
    assert_eq!(v["held"], 2);
    let before = said(&v["unended_before"]);
    assert!(
        before.contains("the broker aborts it, whatever the producer is doing")
            && before.contains(&format!("the epoch goes from {} to {}", t.1, t.1 + 1)),
        "{before}"
    );
    assert_eq!(v["timeout"], "60 s");
    let left = v["left_ms"].as_i64().unwrap();
    assert!((50_000..=60_000).contains(&left), "{left}");
    assert!(
        said(&v["holding"]).contains("orders-1 that is 0"),
        "{}",
        said(&v["holding"])
    );
    assert_eq!(w.end_txn("orders-eos", t, true).await, 0);
    let over = ok(&app, "/api/v1/topics/orders/transactions/orders-eos").await;
    assert_eq!(over["view"]["txn"]["state"], "CompleteCommit");
    assert_eq!(over["held"], 0);
    assert_eq!(
        said(&over["unended_before"]),
        "It is over: nothing is left to abort."
    );
    let (s, none) = get(&app, "/api/v1/topics/orders/transactions/nobody").await;
    assert_eq!(s, StatusCode::OK, "an answer, not a failed request: {none}");
    assert!(none.get("view").is_none());
    assert_eq!(none["missing"]["code"], "TRANSACTIONAL_ID_NOT_FOUND");
    w.stop().await;
}

/// CON-536: an aborted transaction is remembered with its first offset and its marker.
#[tokio::test]
async fn an_aborted_transaction_is_listed_with_its_marker() {
    let w = seeded("al-aborted").await;
    let app = api(&w);
    let t = w.init_producer(Some("tx")).await;
    w.add_to_txn("tx", t, "orders", 0).await;
    assert_eq!(
        w.produce_batch("orders", 0, from(t.0, t.1, 0, true, 2), Some("tx"))
            .await
            .0,
        0
    );
    let v = ok(&app, "/api/v1/topics/orders/producers").await;
    assert!(v["view"]["aborted"].as_array().unwrap().is_empty());
    assert_eq!(w.end_txn("tx", t, false).await, 0);
    let v = ok(&app, "/api/v1/topics/orders/producers").await;
    let a = v["view"]["aborted"].as_array().unwrap();
    assert_eq!(a.len(), 1, "{v}");
    assert_eq!(a[0]["producer"], t.0);
    assert_eq!(a[0]["first"], 0);
    assert_eq!(a[0]["marker"], 2);
    assert!(a[0]["at_ms"].as_i64().is_some());
    assert!(v["view"]["open"].as_array().unwrap().is_empty());
    assert_eq!(v["held"], 0);
    w.stop().await;
}
