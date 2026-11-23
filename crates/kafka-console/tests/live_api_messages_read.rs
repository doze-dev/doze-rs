//! J37: how the Messages answer reads, over a real broker in this process. A key is searched back from the end a thousand
//! offsets at a time and says how much it looked at; a time finds each partition's first record at or after it; an offset is
//! read from on every partition, each with its share of the page; the newest records are found again further back when a
//! transaction's records are left out; a read stops at 4 MiB and still shows the newest records; aborted records stand as
//! aborted; the head names the producers, the open transaction and what each partition holds; the topics list says which
//! topic has a transaction open, which is empty, which has segments to speak of, and that it stops at 200 rows.
//! The mutation run over `format/read.rs` and `gather/{walk,decode,read,records,messages,topic,topics}.rs` found each of
//! these unchecked.
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
use bytes::Bytes;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use serde_json::{Value, json};
use support_api::{api, get, post};
use support_log::{from, keyed, records};
use support_write::{World, world};

async fn ok(app: &Router, path: &str) -> Value {
    let (s, v) = get(app, path).await;
    assert_eq!(s, StatusCode::OK, "{path}: {v}");
    v
}

/// `(partition, offset)` of each row, as shown.
fn shown(v: &Value) -> Vec<(i64, i64)> {
    v["view"]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .map(|r| {
            (
                r["partition"].as_i64().unwrap(),
                r["offset"].as_i64().unwrap(),
            )
        })
        .collect()
}

fn reach(v: &Value) -> &Value {
    &v["view"]["reach"]
}

/// One record with key `key` written at `ts`.
fn at(ts: i64, key: &str) -> Bytes {
    BatchBuilder::from_records(
        ts,
        &[Rec {
            key: Some(key.as_bytes().to_vec()),
            value: Some(b"v".to_vec()),
            ..Rec::default()
        }],
    )
    .build()
}

async fn put(w: &World, topic: &str, partition: i32, batch: Bytes) {
    assert_eq!(w.produce_batch(topic, partition, batch, None).await.0, 0);
}

/// A key is matched whole (not as a prefix, and by its true length), searched back from the end a thousand offsets at a
/// time: the answer says how many records it looked at and whether it looked at all of them. A search that has enough
/// stops, says it did not look at everything, and that more matched.
#[tokio::test]
async fn a_key_is_searched_back_from_the_end_a_thousand_offsets_at_a_time() {
    let w = world("msg-key", |_, _| {}).await;
    w.create_topic("k", 1, &[]).await;
    put(&w, "k", 0, keyed(&[("old", Some("o1"))])).await;
    put(&w, "k", 0, records(1500)).await;
    put(
        &w,
        "k",
        0,
        keyed(&[
            ("new", Some("n1")),
            ("new", Some("n2")),
            ("other", Some("x")),
            ("ne", Some("short")),
        ]),
    )
    .await;
    let app = api(&w);

    let v = ok(&app, "/api/v1/topics/k/messages?key=new").await;
    assert_eq!(shown(&v), vec![(0, 1502), (0, 1501)], "{v}");
    assert!(
        v["view"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["key"]["text"] == "new")
    );
    assert_eq!(reach(&v)["scanned"], json!([1505, true]), "{v}");
    assert_eq!(reach(&v)["returned"], 2);
    assert_eq!(reach(&v)["reached_end"], true);
    assert_eq!(reach(&v)["more"], false, "{v}");

    let v = ok(&app, "/api/v1/topics/k/messages?key=new&limit=1").await;
    assert_eq!(shown(&v), vec![(0, 1502)], "{v}");
    assert_eq!(
        reach(&v)["scanned"],
        json!([1000, false]),
        "one window had enough: {v}"
    );
    assert_eq!(reach(&v)["more"], true, "{v}");

    let v = ok(&app, "/api/v1/topics/k/messages?key=ne").await;
    assert_eq!(shown(&v), vec![(0, 1504)], "not a prefix: {v}");
    let v = ok(&app, "/api/v1/topics/k/messages?key=old").await;
    assert_eq!(shown(&v), vec![(0, 0)], "{v}");
    assert_eq!(reach(&v)["scanned"], json!([1505, true]));
    w.stop().await;
}

/// A read stops once 4 MiB of records have come. The newest records are still shown: what the bound left out is read again
/// nearer the end. A key search that has looked at 4 MiB stops and says it did not look at everything.
#[tokio::test]
async fn a_read_stops_at_four_mebibytes_and_says_so() {
    let w = world("msg-big", |_, _| {}).await;
    w.create_topic("big", 1, &[]).await;
    put(&w, "big", 0, records(1200)).await;
    let large = "x".repeat(1_000_000);
    for _ in 0..6 {
        put(&w, "big", 0, keyed(&[("b", Some(large.as_str()))])).await;
    }
    let app = api(&w);
    // The newest 25 begin 4 MiB short of the end: what the bound left out is read again nearer the end, so the newest are
    // shown (debt J37: the newest record was missing).
    let v = ok(&app, "/api/v1/topics/big/messages").await;
    assert_eq!(
        shown(&v),
        (1181..=1205).rev().map(|o| (0, o)).collect::<Vec<_>>(),
        "{}",
        reach(&v)
    );
    assert_eq!(reach(&v)["reached_end"], true, "{}", reach(&v));
    let v = ok(&app, "/api/v1/topics/big/messages?key=zzz").await;
    assert!(shown(&v).is_empty());
    assert_eq!(
        reach(&v)["scanned"][1],
        false,
        "4 MiB looked at, the rest not: {}",
        reach(&v)
    );
    w.stop().await;
}

/// A time finds each partition's first record at or after it (a partition with none is not read); an offset is read from
/// on every partition, each with its share of the page; a partition the topic does not have is said and left out.
#[tokio::test]
async fn a_time_or_an_offset_is_read_from_on_every_partition_with_its_share() {
    let w = world("msg-time", |_, _| {}).await;
    w.create_topic("t", 3, &[]).await;
    for ts in [10_000, 20_000, 30_000, 40_000] {
        put(&w, "t", 0, at(ts, "a")).await;
    }
    for ts in [15_000, 25_000, 35_000] {
        put(&w, "t", 1, at(ts, "b")).await;
    }
    put(&w, "t", 2, at(5_000, "c")).await;
    let app = api(&w);

    let v = ok(&app, "/api/v1/topics/t/messages?time=20000").await;
    assert_eq!(
        shown(&v),
        vec![(0, 3), (1, 2), (0, 2), (1, 1), (0, 1)],
        "{v}"
    );
    assert_eq!(reach(&v)["reached_end"], true);

    let v = ok(&app, "/api/v1/topics/t/messages?offset=0").await;
    assert_eq!(shown(&v).len(), 8, "{v}");
    assert_eq!(reach(&v)["reached_end"], true, "{}", reach(&v));
    assert_eq!(reach(&v)["more"], false);
    let v = ok(&app, "/api/v1/topics/t/messages?offset=0&limit=2").await;
    assert_eq!(
        shown(&v),
        vec![(1, 0), (0, 0)],
        "one each, the newest two: {v}"
    );
    assert_eq!(reach(&v)["reached_end"], false, "{}", reach(&v));
    assert_eq!(reach(&v)["more"], true);

    let v = ok(&app, "/api/v1/topics/t").await;
    assert_eq!(v["records"], 8, "{v}");
    let v = ok(&app, "/api/v1/topics/t/messages").await;
    assert_eq!(shown(&v).len(), 8);
    assert_eq!(reach(&v)["reached_end"], true);
    assert_eq!(reach(&v)["more"], false, "every record is shown");

    let v = ok(&app, "/api/v1/topics/t/messages?partition=0&partition=9").await;
    assert_eq!(
        v["view"]["notes"],
        json!(["t has no partition 9, so it is left out."])
    );
    assert!(shown(&v).iter().all(|(p, _)| *p == 0), "{v}");
    let v = ok(&app, "/api/v1/topics/t/messages?partition=8&partition=9").await;
    assert_eq!(
        v["view"]["notes"],
        json!(["t has no partition 8, 9, so they are left out."])
    );
    assert_eq!(shown(&v).len(), 8, "none left: every partition");
    let v = ok(&app, "/api/v1/topics/t/messages?partition=1").await;
    assert!(v["view"]["notes"].as_array().unwrap().is_empty(), "{v}");
    assert_eq!(shown(&v), vec![(1, 2), (1, 1), (1, 0)]);
    w.stop().await;
}

/// Records of an aborted transaction stand as aborted for read_uncommitted and are left out for read_committed, which reads
/// further back for the newest ones it may show. The head names the producers, the open transaction with the first offset
/// it holds on each partition, and each partition's bytes; each partition says what holds it back. The topics list says
/// which topic has a transaction open, which is empty and which has more segments than partitions.
#[tokio::test]
async fn transactions_stand_where_a_consumer_would_and_the_head_says_who_holds_what() {
    let w = world("msg-txn", |_, _| {}).await;
    w.create_topic("x", 1, &[]).await;
    put(&w, "x", 0, records(10)).await;
    let ab = w.init_producer(Some("ab")).await;
    w.add_to_txn("ab", ab, "x", 0).await;
    assert_eq!(
        w.produce_batch("x", 0, from(ab.0, ab.1, 0, true, 30), Some("ab"))
            .await
            .0,
        0
    );
    assert_eq!(w.end_txn("ab", ab, false).await, 0);
    let app = api(&w);

    let v = ok(&app, "/api/v1/topics/x/messages").await;
    let first = &v["view"]["rows"][0];
    assert_eq!(
        (&first["offset"], &first["standing"]),
        (&json!(39), &json!("aborted")),
        "{v}"
    );
    let v = ok(
        &app,
        "/api/v1/topics/x/messages?isolation=read_committed&limit=10&before=0:41",
    )
    .await;
    assert_eq!(
        shown(&v),
        (0..10).rev().map(|o| (0, o)).collect::<Vec<_>>(),
        "back past the aborted ones to the newest ten committed: {v}"
    );
    assert!(
        v["view"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["standing"] == "committed")
    );
    assert_eq!(reach(&v)["more"], false, "nothing older is left: {v}");
    let v = ok(&app, "/api/v1/topics/x/messages?limit=25&before=0:30").await;
    assert_eq!(shown(&v).first(), Some(&(0, 29)), "{v}");
    assert_eq!(reach(&v)["more"], true, "older records are left: {v}");

    w.create_topic("t2", 3, &[]).await;
    let idem = w.init_producer(None).await;
    assert_eq!(
        w.produce_batch("t2", 0, from(idem.0, idem.1, 0, false, 2), None)
            .await
            .0,
        0
    );
    let open = w.init_producer(Some("open")).await;
    w.add_to_txn("open", open, "t2", 0).await;
    w.add_to_txn("open", open, "t2", 1).await;
    assert_eq!(
        w.produce_batch("t2", 1, from(open.0, open.1, 0, true, 3), Some("open"))
            .await
            .0,
        0
    );
    let v = ok(&app, "/api/v1/topics/t2/messages").await;
    let head = &v["view"]["head"];
    assert_eq!(head["tabs"]["producers"], 2, "{head}");
    let txns = head["txns"].as_array().unwrap();
    assert_eq!(txns.len(), 1, "{head}");
    assert_eq!(txns[0]["id"], "open");
    assert_eq!(
        txns[0]["partitions"],
        json!([[0, null], [1, 0]]),
        "it holds partition 1 from 0, and nothing on 0: {head}"
    );
    let bytes: Vec<i64> = head["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["bytes"].as_i64().unwrap_or_else(|| panic!("{p}")))
        .collect();
    assert!(bytes[0] > 0 && bytes[1] > 0 && bytes[2] == 0, "{bytes:?}");
    let holds: Vec<Value> = v["view"]["holds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            json!([
                h["partition"],
                h["high_watermark"],
                h["last_stable"],
                h["txn"]
            ])
        })
        .collect();
    assert_eq!(
        holds,
        vec![
            json!([0, 2, 2, null]),
            json!([1, 3, 0, ["open", 0]]),
            json!([2, 0, 0, null])
        ]
    );
    assert_eq!(reach(&v)["reached_end"], true);
    assert_eq!(reach(&v)["more"], false, "{v}");
    assert_eq!(
        reach(&v)["above_lso"],
        3,
        "the open transaction's three: {v}"
    );
    let v = ok(&app, "/api/v1/topics/t2/messages?isolation=read_committed").await;
    assert_eq!(shown(&v).len(), 2, "{v}");
    assert_eq!(
        reach(&v)["more"],
        true,
        "five records, two of them readable now"
    );

    w.create_topic("e", 1, &[]).await;
    w.create_topic("seg", 1, &[("segment.bytes", "1024")]).await;
    w.fill("seg", 40, 4).await;
    let v = ok(&app, "/api/v1/topics").await;
    let row = |name: &str| {
        v["view"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap_or_else(|| panic!("no {name}: {v}"))
            .clone()
    };
    assert_eq!(row("t2")["txn_open"], true);
    assert_eq!(row("x")["txn_open"], false, "its transaction is over");
    assert_eq!(row("t2")["empty"], false);
    assert!(row("t2")["bytes"].as_i64().unwrap() > 0);
    assert_eq!(row("e")["empty"], true);
    assert_eq!(row("e")["bytes"], 0);
    assert_eq!(row("e")["segments"], Value::Null, "nothing written");
    assert_eq!(
        row("x")["segments"],
        Value::Null,
        "one segment, one partition"
    );
    assert!(
        row("seg")["segments"].as_i64().unwrap() > 1,
        "{}",
        row("seg")
    );
    assert_eq!(v["view"]["truncated"], false);
    let v = ok(&app, "/api/v1/topics?q=t2").await;
    let names: Vec<&str> = v["view"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["t2"]);
    w.stop().await;
}

/// The topics list shows at most 200 rows and says when it stopped; the Delete records dialog of a partition whose records
/// are all deleted starts from where its log starts now.
#[tokio::test]
async fn the_list_stops_at_two_hundred_and_a_deleted_log_starts_where_it_was_cut() {
    let w = world("msg-many", |_, _| {}).await;
    for i in 0..200 {
        w.create_topic(&format!("n{i:03}"), 1, &[]).await;
    }
    let app = api(&w);
    let v = ok(&app, "/api/v1/topics").await;
    assert_eq!(v["view"]["rows"].as_array().unwrap().len(), 200);
    assert_eq!(v["view"]["truncated"], false, "200 is all of them");
    w.create_topic("n200", 1, &[]).await;
    let v = ok(&app, "/api/v1/topics").await;
    assert_eq!(v["view"]["rows"].as_array().unwrap().len(), 200);
    assert_eq!(v["view"]["truncated"], true);
    assert_eq!(v["view"]["total"], 201);

    put(&w, "n000", 0, records(3)).await;
    let (s, v) = post(
        &app,
        "/api/v1/topics/n000/delete-records",
        &[("partition", "0"), ("offset", "3")],
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let v = ok(&app, "/api/v1/topics/n000/delete-records?partition=0").await;
    assert_eq!(v["view"]["log_start"], 3, "{v}");
    assert_eq!(v["view"]["high_watermark"], 3);
    w.stop().await;
}
