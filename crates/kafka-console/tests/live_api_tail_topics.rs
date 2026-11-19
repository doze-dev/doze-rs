//! The Live tail page's stream (`GET /api/v1/tail/topics`, CON-1580 to CON-1585) over a real broker in this process: several
//! topics merged, from now or the last N of each partition, a pattern that a topic created later joins, the filters applied
//! by the server with matched and scanned counted, a reconnect that goes on from its cursor with nothing twice, a partition
//! far behind moved to its newest, and the asks it refuses.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_api;
mod support_log;
mod support_schemas;
mod support_write;

use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use serde_json::Value;
use support::HOST;
use support_api::api;
use support_log::records;
use support_write::{World, world};
use tower::ServiceExt;

/// An open tail: the body of the answer, read event by event.
struct Tail {
    body: Body,
    text: String,
}

/// One event: its name, its id and its data.
#[derive(Debug)]
struct Ev {
    name: String,
    id: Option<String>,
    data: Value,
}

async fn open(app: &Router, query: &str, last_id: Option<&str>) -> Tail {
    let mut req = Request::builder()
        .uri(format!("/api/v1/tail/topics?{query}"))
        .header(header::HOST, HOST)
        .header("x-console", "1");
    if let Some(id) = last_id {
        req = req.header("last-event-id", id);
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    Tail {
        body: res.into_body(),
        text: String::new(),
    }
}

impl Tail {
    /// The next event that is not a keep-alive, within `within`.
    async fn next(&mut self, within: Duration) -> Option<Ev> {
        let deadline = Instant::now() + within;
        loop {
            if let Some(i) = self.text.find("\n\n") {
                let block: String = self.text.drain(..i + 2).collect();
                let (mut name, mut id, mut data) = (String::from("message"), None, String::new());
                for line in block.lines() {
                    if let Some(v) = line.strip_prefix("event:") {
                        name = v.trim().to_owned();
                    } else if let Some(v) = line.strip_prefix("id:") {
                        id = Some(v.trim().to_owned());
                    } else if let Some(v) = line.strip_prefix("data:") {
                        data.push_str(v.trim_start());
                    }
                }
                if data.is_empty() || data == "keep-alive" {
                    continue;
                }
                let data = serde_json::from_str(&data).unwrap_or(Value::String(data));
                return Some(Ev { name, id, data });
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let frame = tokio::time::timeout(left, self.body.frame()).await.ok()??;
            let bytes = frame.unwrap().into_data().unwrap_or_default();
            self.text.push_str(&String::from_utf8_lossy(&bytes));
        }
    }

    async fn ready(&mut self) -> Value {
        let ev = self.next(Duration::from_secs(5)).await.expect("ready");
        assert_eq!(ev.name, "ready", "{ev:?}");
        ev.data
    }

    /// The next batch with rows, within 8 s.
    async fn rows(&mut self) -> Ev {
        loop {
            let ev = self
                .next(Duration::from_secs(8))
                .await
                .expect("a batch with rows");
            if ev.name == "batch" && !ev.data["rows"].as_array().unwrap().is_empty() {
                return ev;
            }
        }
    }
}

fn places(rows: &Value) -> Vec<(String, i64, i64)> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["topic"].as_str().unwrap().to_owned(),
                r["partition"].as_i64().unwrap(),
                r["offset"].as_i64().unwrap(),
            )
        })
        .collect()
}

async fn put(w: &World, topic: &str, partition: i32, recs: &[Rec]) {
    let b = BatchBuilder::from_records(1_700_000_000_000, recs).build();
    assert_eq!(w.produce_batch(topic, partition, b, None).await.0, 0);
}

fn rec(key: &str, value: &str, headers: &[(&str, &str)]) -> Rec {
    Rec {
        key: Some(key.as_bytes().to_vec()),
        value: Some(value.as_bytes().to_vec()),
        headers: headers
            .iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect(),
        ..Rec::default()
    }
}

#[tokio::test]
async fn two_topics_from_now_merge_what_is_written_after_the_tail_opened() {
    let w = world("tail-two", |_, _| {}).await;
    w.create_topic("orders", 2, &[]).await;
    w.create_topic("payments", 1, &[]).await;
    put(&w, "orders", 0, &[rec("old", "before", &[])]).await;
    let app = api(&w);
    let mut t = open(&app, "topic=orders&topic=payments&pace=1", None).await;
    let ready = t.ready().await;
    assert_eq!(ready["topics"][0]["name"], "orders");
    assert_eq!(ready["topics"][0]["partitions"], 2);
    assert_eq!(ready["topics"][1]["name"], "payments");
    assert_eq!(ready["cursor"], "orders:0=1,1=0;payments:0=0");
    put(
        &w,
        "orders",
        1,
        &[rec("o-1", r#"{"n":1}"#, &[("trace", "t-1")])],
    )
    .await;
    put(&w, "payments", 0, &[rec("p-1", "paid", &[])]).await;
    let mut seen = Vec::new();
    let mut all = Vec::new();
    let mut last = None;
    while seen.len() < 2 {
        let ev = t.rows().await;
        assert_eq!(
            ev.data["scanned"], ev.data["matched"],
            "no filter: all kept"
        );
        seen.extend(places(&ev.data["rows"]));
        all.extend(ev.data["rows"].as_array().unwrap().iter().cloned());
        last = Some(ev);
    }
    seen.sort();
    assert_eq!(
        seen,
        [("orders".into(), 1, 0), ("payments".into(), 0, 0)],
        "nothing from before"
    );
    let last = last.unwrap();
    assert_eq!(last.id.as_deref(), last.data["cursor"].as_str());
    assert!(last.id.unwrap().contains("payments:0=1"));
    // A record carries its key, its value as the Messages table shows it, and its headers.
    let o = all.iter().find(|r| r["topic"] == "orders").unwrap();
    assert_eq!(o["key"]["text"], "o-1");
    assert_eq!(o["value"]["kind"], "json");
    assert_eq!(o["headers"][0]["key"], "trace");
    assert_eq!(o["headers"][0]["value"]["text"], "t-1");
    assert_eq!(o["headers_total"], 1);
    w.stop().await;
}

#[tokio::test]
async fn last_n_starts_with_the_newest_of_each_partition_newest_first() {
    let w = world("tail-last", |_, _| {}).await;
    w.create_topic("orders", 2, &[]).await;
    for p in 0..2 {
        assert_eq!(w.produce_batch("orders", p, records(10), None).await.0, 0);
    }
    let app = api(&w);
    let mut t = open(&app, "topic=orders&from=last&last=3&pace=1", None).await;
    t.ready().await;
    let ev = t.rows().await;
    let mut got = places(&ev.data["rows"]);
    assert_eq!(got.len(), 6, "{got:?}");
    // Within a partition, newest first.
    let p0: Vec<i64> = got.iter().filter(|r| r.1 == 0).map(|r| r.2).collect();
    assert_eq!(p0, [9, 8, 7]);
    got.sort();
    assert_eq!(got.first().unwrap(), &("orders".into(), 0, 7));
    assert_eq!(ev.data["counts"][0]["topic"], "orders");
    assert_eq!(ev.data["counts"][0]["scanned"], 6);
    w.stop().await;
}

#[tokio::test]
async fn a_topic_created_later_that_matches_the_pattern_joins_from_its_start() {
    let w = world("tail-pattern", |_, _| {}).await;
    w.create_topic("orders.eu", 1, &[]).await;
    w.create_topic("billing", 1, &[]).await;
    let app = api(&w);
    let mut t = open(&app, "pattern=orders.*&pace=1", None).await;
    let ready = t.ready().await;
    assert_eq!(ready["topics"].as_array().unwrap().len(), 1, "{ready}");
    w.create_topic("orders.us", 1, &[]).await;
    put(&w, "orders.us", 0, &[rec("u-1", "first", &[])]).await;
    put(&w, "billing", 0, &[rec("b-1", "not followed", &[])]).await;
    let mut joined = false;
    let mut rows = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while rows.is_empty() && Instant::now() < deadline {
        let ev = t.next(Duration::from_secs(8)).await.expect("a batch");
        joined |= ev.data["joined"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["name"] == "orders.us");
        rows.extend(places(&ev.data["rows"]));
    }
    assert!(joined, "the new topic is said to have joined");
    assert_eq!(
        rows,
        [("orders.us".into(), 0, 0)],
        "all of it, and no other topic"
    );
    // A pattern that matches nothing yet says so, and is still a tail.
    let mut none = open(&app, "pattern=nothing.*", None).await;
    let r = none.ready().await;
    assert!(
        r["notes"][0]
            .as_str()
            .unwrap()
            .contains("No topic matches yet"),
        "{r}"
    );
    w.stop().await;
}

#[tokio::test]
async fn the_filters_run_on_the_server_and_say_what_they_matched_of_what_they_read() {
    let w = world("tail-filter", |_, _| {}).await;
    w.create_topic("orders", 2, &[]).await;
    let app = api(&w);
    let cases = [
        ("key=ORD-1", vec!["ord-1"]),
        ("value=declined", vec!["ord-2"]),
        ("header=trace&header_value=ABC", vec!["ord-3"]),
        ("header=trace", vec!["ord-1", "ord-3"]),
        ("field=customer.id&equals=c-7", vec!["ord-2"]),
        ("field=items[0].sku&equals=A-1", vec!["ord-1"]),
        ("field=total&equals=12.5", vec!["ord-3"]),
        ("partition=1", vec!["ord-3"]),
    ];
    let mut tails = Vec::new();
    for (q, _) in &cases {
        let mut t = open(&app, &format!("topic=orders&pace=1&{q}"), None).await;
        t.ready().await;
        tails.push(t);
    }
    put(
        &w,
        "orders",
        0,
        &[
            rec(
                "ord-1",
                r#"{"items":[{"sku":"A-1"}],"state":"paid"}"#,
                &[("trace", "x-1")],
            ),
            rec(
                "ord-2",
                r#"{"customer":{"id":"c-7"},"state":"DECLINED"}"#,
                &[],
            ),
        ],
    )
    .await;
    put(
        &w,
        "orders",
        1,
        &[rec("ord-3", r#"{"total":12.5}"#, &[("trace", "abc-9")])],
    )
    .await;
    for ((q, want), t) in cases.iter().zip(tails.iter_mut()) {
        let mut keys = Vec::new();
        let (mut scanned, mut matched) = (0, 0);
        let deadline = Instant::now() + Duration::from_secs(10);
        while scanned < if q.starts_with("partition") { 1 } else { 3 } && Instant::now() < deadline
        {
            let ev = t.next(Duration::from_secs(8)).await.expect("a batch");
            scanned += ev.data["scanned"].as_u64().unwrap();
            matched += ev.data["matched"].as_u64().unwrap();
            for r in ev.data["rows"].as_array().unwrap() {
                keys.push(r["key"]["text"].as_str().unwrap().to_owned());
            }
        }
        keys.sort();
        assert_eq!(&keys, want, "{q}");
        assert_eq!(matched, want.len() as u64, "{q}");
    }
    w.stop().await;
}

#[tokio::test]
async fn a_reconnect_with_the_cursor_brings_what_it_missed_and_nothing_twice() {
    let w = world("tail-resume", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    let app = api(&w);
    let mut t = open(&app, "topic=orders&pace=1", None).await;
    t.ready().await;
    put(&w, "orders", 0, &[rec("a", "1", &[]), rec("b", "2", &[])]).await;
    let first = t.rows().await;
    assert_eq!(places(&first.data["rows"]).len(), 2);
    let cursor = first.id.unwrap();
    assert_eq!(cursor, "orders:0=2");
    drop(t);
    // While the page was away.
    put(&w, "orders", 0, &[rec("c", "3", &[])]).await;
    let mut again = open(&app, "topic=orders&pace=1", Some(&cursor)).await;
    let ready = again.ready().await;
    assert_eq!(ready["cursor"], "orders:0=2", "it goes on from the cursor");
    let ev = again.rows().await;
    assert_eq!(places(&ev.data["rows"]), [("orders".into(), 0, 2)]);
    w.stop().await;
}

#[tokio::test]
async fn a_partition_far_behind_is_moved_to_its_newest_and_what_it_passed_over_is_counted() {
    let w = world("tail-skip", |_, _| {}).await;
    w.create_topic("busy", 1, &[]).await;
    for _ in 0..5 {
        assert_eq!(w.produce_batch("busy", 0, records(5_000), None).await.0, 0);
    }
    let app = api(&w);
    let mut t = open(&app, "topic=busy&pace=1", Some("busy:0=0")).await;
    t.ready().await;
    let ev = t.next(Duration::from_secs(8)).await.expect("a batch");
    assert_eq!(ev.data["skipped"], 25_000 - 1_000, "{}", ev.data["skipped"]);
    // One look reads what its budget allows, and an event carries at most 200 rows.
    assert!(ev.data["rows"].as_array().unwrap().len() <= 200);
    assert!(ev.data["scanned"].as_u64().unwrap() <= 1_000);
    w.stop().await;
}

#[tokio::test]
async fn an_ask_that_cannot_be_followed_is_refused_in_words() {
    let w = world("tail-refuse", |_, _| {}).await;
    let app = api(&w);
    for (q, words) in [
        ("", "Name a topic to follow"),
        ("pattern=a%20b", "A pattern is"),
        ("topic=orders&field=a..b", "A JSON field is a path"),
        ("topic=no%20such", "cannot be a topic name"),
    ] {
        let req = Request::builder()
            .uri(format!("/api/v1/tail/topics?{q}"))
            .header(header::HOST, HOST)
            .header("x-console", "1")
            .body(Body::empty())
            .unwrap();
        let (res, body) = support::send(&app, req).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{q}");
        assert!(body.contains(words), "{q}: {body}");
    }
    // A topic that does not exist yet is not refused: it joins when it is made.
    let mut t = open(&app, "topic=later", None).await;
    let r = t.ready().await;
    assert!(
        r["notes"][0].as_str().unwrap().contains("joins the tail"),
        "{r}"
    );
    w.stop().await;
}

#[tokio::test]
async fn a_value_with_a_schema_is_filtered_and_shown_as_its_schema_reads_it() {
    let w = world("tail-schema", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    let (app, r) = support_schemas::with_registry(&w);
    support_schemas::post_json(
        &app,
        "/api/v1/schemas/register",
        &support_schemas::draft("orders-value", "AVRO", support_schemas::V2),
    )
    .await;
    let avro = r.codec_for("orders-value", "latest").unwrap().0;
    let mut all = open(&app, "topic=orders&pace=1", None).await;
    all.ready().await;
    let mut t = open(&app, "topic=orders&pace=1&field=note&equals=rush", None).await;
    t.ready().await;
    for (id, note) in [(7, "rush"), (8, "slow")] {
        let v = avro
            .encode(&format!(r#"{{"id":{id},"note":"{note}"}}"#))
            .unwrap();
        support_schemas::produce_value(&w, "orders", v).await;
    }
    // Unfiltered, both; filtered on the decoded field, the one.
    let every = all.rows().await;
    assert_eq!(every.data["rows"].as_array().unwrap().len(), 2);
    let ev = t.rows().await;
    let rows = ev.data["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    let k = &rows[0]["value"]["schema"];
    assert_eq!(k["kind"], "AVRO");
    assert_eq!(k["subject"], "orders-value");
    assert!(k["preview"].as_str().unwrap().contains("rush"), "{k}");
    assert_eq!(k["json"], Value::Null, "the laid-out JSON is the Sheet's");
    w.stop().await;
}
