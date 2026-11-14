//! The Log, Producers and transaction answers of the Topics API over a real broker in this process (the rules CON-530 to
//! CON-537 and CON-539 that drove the server-rendered pages before R5): what the Log says is what the broker's log holds and
//! what the cleaner then does; a transaction held open is the one that holds the last stable offset; an abort is remembered
//! with its marker; a caller without rights meets what a client would; and a GET changes nothing.
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
use kafka_wire::messages::ApiKey;
use serde_json::Value;
use support_api::{api, get};
use support_log::{from, keyed, records, seeded};
use support_write::world;

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

fn titles(rules: &Value) -> Vec<String> {
    rules
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap().to_owned())
        .collect()
}

/// CON-530: a compacted log that is due says so with what a round would rewrite; after the cleaner has run, the answer has the
/// round (when, what it read, kept and replaced), the ruler hatches what it removed, the deletion it kept is listed with when it
/// goes, and the numbers are the broker's.
#[tokio::test]
async fn a_compacted_log_is_drawn_from_what_the_cleaner_left() {
    let w = seeded("al-compacted").await;
    let app = api(&w);
    assert_eq!(
        w.produce_batch("inventory.levels", 0, keyed(&[("gone", Some("1"))]), None)
            .await
            .0,
        0
    );
    assert_eq!(
        w.produce_batch("inventory.levels", 0, keyed(&[("gone", None)]), None)
            .await
            .0,
        0
    );
    w.fill("inventory.levels", 60, 5).await;
    let due = ok(&app, "/api/v1/topics/inventory.levels/log").await;
    assert_eq!(due["state"], "due", "{due}");
    assert_eq!(due["view"]["compaction"]["planner"]["due"], true);
    assert!(due["view"]["compaction"]["last"].is_null(), "no round yet");
    assert!(due["round"].is_null());
    assert!(!due["would_rewrite"].as_array().unwrap().is_empty());
    assert!(
        due["ruler"]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["gaps"].as_array().unwrap().is_empty()),
        "nothing is removed before a round: {}",
        due["ruler"]
    );
    w.broker.check_cleaner().await;
    w.fill("inventory.levels", 30, 5).await;
    let after = ok(&app, "/api/v1/topics/inventory.levels/log").await;
    let last = &after["view"]["compaction"]["last"];
    assert!(last.is_object(), "the round: {after}");
    assert!(last["replaced"].as_i64().unwrap() > 0, "{last}");
    assert!(
        after["round"]
            .as_str()
            .unwrap()
            .ends_with("on inventory.levels-0."),
        "{}",
        after["round"]
    );
    assert!(
        after["ruler"]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["cell"] == "clean" && !c["gaps"].as_array().unwrap().is_empty()),
        "the ruler hatches what the round removed: {}",
        after["ruler"]
    );
    let tomb = &after["view"]["tombstones"][0];
    assert_eq!(tomb["key"], "gone", "the deletion is listed: {after}");
    assert_eq!(tomb["offset"], 1);
    assert!(tomb["goes_after_ms"].as_i64().is_some());
    // The numbers are the broker's: the end offset, and as many segments as the walk found.
    let end = w.offset("inventory.levels", 0, -1).await;
    assert_eq!(end, 92);
    assert_eq!(after["view"]["end"], end);
    let facts = w.backend.log_facts("inventory.levels", 0).await.unwrap();
    assert_eq!(
        after["view"]["segments"].as_array().unwrap().len(),
        facts.segments.len()
    );
    assert_eq!(
        after["ruler"]["marks"].as_array().unwrap().last().unwrap()["offset"],
        end
    );
    w.stop().await;
}

/// CON-531: a compacted log that is not due says why (the planner's rules, what is still to come) and the cleaner then
/// leaves it alone.
#[tokio::test]
async fn a_log_that_is_not_due_says_why_and_the_cleaner_then_leaves_it_alone() {
    let w = world("al-notdue", |_, _| {}).await;
    w.create_topic(
        "customer.profiles",
        1,
        &[
            ("cleanup.policy", "compact"),
            ("segment.bytes", "1024"),
            ("min.cleanable.dirty.ratio", "0.5"),
        ],
    )
    .await;
    w.fill("customer.profiles", 80, 40).await;
    w.broker.check_cleaner().await;
    w.fill("customer.profiles", 16, 40).await;
    let app = api(&w);
    let before = w.backend.log_facts("customer.profiles", 0).await.unwrap();
    let v = ok(&app, "/api/v1/topics/customer.profiles/log").await;
    assert_eq!(v["state"], "not_due", "{v}");
    assert_eq!(v["view"]["compaction"]["planner"]["due"], false);
    let to_come = &v["to_come"];
    assert!(to_come.is_object(), "what would make it due: {v}");
    assert!(!said(&to_come["coming"]).is_empty(), "{to_come}");
    assert!(
        titles(&v["rules"]).iter().any(|t| t.contains("dirty")),
        "the rule it fails: {}",
        v["rules"]
    );
    assert!(v["would_rewrite"].as_array().unwrap().is_empty());
    w.broker.check_cleaner().await;
    let after = w.backend.log_facts("customer.profiles", 0).await.unwrap();
    assert_eq!(
        before
            .segments
            .iter()
            .map(|s| (s.base, s.bytes))
            .collect::<Vec<_>>(),
        after
            .segments
            .iter()
            .map(|s| (s.base, s.bytes))
            .collect::<Vec<_>>(),
        "not due means nothing changed"
    );
    w.stop().await;
}

/// CON-532: a log that deletes says what the next pass will do (the rules with its numbers, what goes), and after a pass what
/// the pass did (the log start that moved, from and to).
#[tokio::test]
async fn retention_is_named_before_and_after_a_pass() {
    let w = world("al-retention", |_, _| {}).await;
    w.create_topic(
        "orders",
        1,
        &[("segment.bytes", "1024"), ("retention.bytes", "3000")],
    )
    .await;
    for _ in 0..60 {
        assert_eq!(w.produce_batch("orders", 0, records(5), None).await.0, 0);
    }
    let app = api(&w);
    let before = ok(&app, "/api/v1/topics/orders/log").await;
    assert_eq!(before["state"], "retention");
    let r = &before["view"]["retention"];
    assert!(r["last_pass"].is_null(), "no pass has run since the broker started");
    assert_eq!(r["max_bytes"], 3000);
    let would = r["would_delete"].as_i64().unwrap();
    assert!(would > 0, "{r}");
    assert!(
        titles(&before["retention_rules"]).contains(&"Size: 2.9 KiB".to_owned()),
        "{}",
        before["retention_rules"]
    );
    let new_start = before["expiry"]["new_start"].as_i64().unwrap();
    let start = w.offset("orders", 0, -2).await;
    w.broker.check_logs().await;
    let moved = w.offset("orders", 0, -2).await;
    assert!(moved > start, "the pass deleted segments");
    assert_eq!(moved, new_start, "the next pass did what the answer said it would");
    let after = ok(&app, "/api/v1/topics/orders/log").await;
    let pass = &after["view"]["retention"]["last_pass"];
    assert_eq!(
        pass["moved"],
        serde_json::json!([{"topic": "orders", "partition": 0, "from": start, "to": moved}]),
        "{pass}"
    );
    assert_eq!(after["view"]["retention"]["moved_total"], 1);
    assert_eq!(after["view"]["start"], moved);
    w.stop().await;
}

/// CON-533: a partition with nothing in it is the empty state: never written, one active cell, the end the first record will
/// take (the page puts the command that fills it there).
#[tokio::test]
async fn an_empty_log_says_how_the_first_record_gets_there() {
    let w = seeded("al-empty").await;
    let app = api(&w);
    let v = ok(&app, "/api/v1/topics/orders.dlq/log").await;
    assert_eq!(v["state"], "empty", "{v}");
    assert_eq!(v["view"]["never_written"], true);
    assert_eq!(v["view"]["end"], 0);
    assert!(v["view"]["segments"].as_array().unwrap().is_empty());
    let cells = v["ruler"]["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0]["cell"], "active");
    // One record and it is a log.
    assert_eq!(w.produce_batch("orders.dlq", 0, records(1), None).await.0, 0);
    let v = ok(&app, "/api/v1/topics/orders.dlq/log").await;
    assert_ne!(v["state"], "empty", "{v}");
    w.stop().await;
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
    assert_eq!(view["last_stable"], end - 3, "the open transaction's first offset");
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
    assert_eq!(batches[4]["first_sequence"], 3, "the two oldest are forgotten");
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
    assert!(said(&v["holding"]).contains("orders-1 that is 0"), "{}", said(&v["holding"]));
    assert_eq!(w.end_txn("orders-eos", t, true).await, 0);
    let over = ok(&app, "/api/v1/topics/orders/transactions/orders-eos").await;
    assert_eq!(over["view"]["txn"]["state"], "CompleteCommit");
    assert_eq!(over["held"], 0);
    assert_eq!(said(&over["unended_before"]), "It is over: nothing is left to abort.");
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

/// CON-537: a caller without rights meets what a client would: no such topic, and no fact about it.
#[tokio::test]
async fn a_caller_without_rights_is_told_no_such_topic() {
    let w = world("al-acl", |bc, _| bc.acl_default_allow = false).await;
    w.create_topic("orders", 1, &[]).await;
    let t = w.init_producer(Some("tx")).await;
    w.add_to_txn("tx", t, "orders", 0).await;
    assert_eq!(
        w.produce_batch("orders", 0, from(t.0, t.1, 0, true, 2), Some("tx"))
            .await
            .0,
        0
    );
    let nobody = kafka_broker::Caller::anonymous(Some("127.0.0.1".parse().unwrap()));
    let app = support_write::router_of(&w, w.backend.acting_as(nobody));
    let (_, never) = get(&app, "/api/v1/topics/never-made/log").await;
    for path in ["/api/v1/topics/orders/log", "/api/v1/topics/orders/producers"] {
        let (status, v) = get(&app, path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {v}");
        assert!(v.get("view").is_none(), "{path}: {v}");
        assert_eq!(v["code"], never["code"], "not told apart from no topic: {v}");
    }
    // The transaction it may not describe is a transaction it is not told of.
    let (_, v) = get(&app, "/api/v1/topics/orders/transactions/tx").await;
    assert!(v.get("view").is_none(), "{v}");
    assert_eq!(v["missing"]["code"], "TRANSACTIONAL_ID_NOT_FOUND", "{v}");
    // The console itself is a super user and sees them.
    let console = api(&w);
    ok(&console, "/api/v1/topics/orders/log").await;
    let v = ok(&console, "/api/v1/topics/orders/transactions/tx").await;
    assert_eq!(v["view"]["txn"]["state"], "Ongoing");
    w.stop().await;
}

/// CON-539: these answers never write: every call a GET makes is a read, and nothing in the broker changes.
#[tokio::test]
async fn a_get_of_these_answers_reads_and_changes_nothing() {
    let w = seeded("al-readonly").await;
    w.fill("inventory.levels", 40, 4).await;
    let t = w.init_producer(Some("tx")).await;
    w.add_to_txn("tx", t, "orders", 0).await;
    assert_eq!(
        w.produce_batch("orders", 0, from(t.0, t.1, 0, true, 2), Some("tx"))
            .await
            .0,
        0
    );
    let (spy, log) = w.backend.recording();
    let app = support_write::router_of(&w, spy);
    let before = (
        w.topics().await,
        w.offset("orders", 0, -1).await,
        w.offset("inventory.levels", 0, -1).await,
        w.backend.log_facts("inventory.levels", 0).await.unwrap().segments.len(),
    );
    for path in [
        "/api/v1/topics/orders/log",
        "/api/v1/topics/orders/log?partition=2",
        "/api/v1/topics/inventory.levels/log",
        "/api/v1/topics/orders.dlq/log",
        "/api/v1/topics/orders/producers",
        "/api/v1/topics/orders/producers?partition=1",
        "/api/v1/topics/orders/transactions/tx",
    ] {
        ok(&app, path).await;
    }
    let calls = log.lock().unwrap().clone();
    assert!(!calls.is_empty(), "the spy saw the reads");
    let reads = [
        ApiKey::Metadata,
        ApiKey::ListTransactions,
        ApiKey::DescribeTransactions,
        ApiKey::ListOffsets,
        ApiKey::DescribeConfigs,
        ApiKey::DescribeProducers,
        ApiKey::ListGroups,
        ApiKey::OffsetFetch,
    ];
    for call in &calls {
        assert!(reads.contains(call), "a GET called {call:?}");
    }
    assert_eq!(
        (
            w.topics().await,
            w.offset("orders", 0, -1).await,
            w.offset("inventory.levels", 0, -1).await,
            w.backend.log_facts("inventory.levels", 0).await.unwrap().segments.len(),
        ),
        before,
        "the log was not compacted, nothing was written"
    );
    w.stop().await;
}
