//! The Messages answer past the most a page shows (debt J27): "Older" doubles the page up to 200 records, and from there a
//! `before` cursor (`partition:offset`, comma separated) reads the next 200, partition by partition, until the log's start.
//! Over a real broker in this process.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_api;
mod support_log;
mod support_write;

use std::collections::BTreeSet;

use axum::http::StatusCode;
use serde_json::Value;
use support_api::{api, get};
use support_log::records;
use support_write::world;

fn offsets(v: &Value) -> Vec<(i64, i64)> {
    v["view"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["partition"].as_i64().unwrap(),
                r["offset"].as_i64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn older_past_two_hundred_records_follows_a_cursor_to_the_start_of_every_partition() {
    let w = world("msg-before", |_, _| {}).await;
    w.create_topic("deep", 2, &[]).await;
    for p in 0..2 {
        for _ in 0..3 {
            assert_eq!(w.produce_batch("deep", p, records(100), None).await.0, 0);
        }
    }
    let app = api(&w);
    // The first page at the most a page shows: no larger page to ask for, so the cursor.
    let (s, first) = get(&app, "/api/v1/topics/deep/messages?limit=200").await;
    assert_eq!(s, StatusCode::OK, "{first}");
    assert_eq!(first["older"], Value::Null, "{first}");
    assert_eq!(first["tailable"], true);
    let mut seen: BTreeSet<(i64, i64)> = BTreeSet::new();
    let mut page = first;
    let mut pages = 0;
    loop {
        let rows = offsets(&page);
        assert!(rows.len() <= 200);
        for r in &rows {
            assert!(seen.insert(*r), "{r:?} shown twice");
        }
        pages += 1;
        let Some(cursor) = page["earlier"].as_str().map(ToOwned::to_owned) else {
            break;
        };
        assert!(pages < 10, "the cursor never reached the start");
        let (s, next) = get(
            &app,
            &format!("/api/v1/topics/deep/messages?limit=200&before={cursor}"),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        // A page read before a cursor is no tail, and its query carries the cursor back.
        assert_eq!(next["tailable"], false);
        assert!(
            !next["view"]["query"]["before"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        // Each record is older than the cursor of its partition.
        for (p, o) in offsets(&next) {
            let bound = cursor
                .split(',')
                .find_map(|pair| {
                    let (cp, co) = pair.split_once(':').unwrap();
                    (cp.parse::<i64>().unwrap() == p).then(|| co.parse::<i64>().unwrap())
                })
                .unwrap_or(i64::MAX);
            assert!(o < bound, "{p}:{o} is not before {bound}");
        }
        page = next;
    }
    assert_eq!(seen.len(), 600, "every record of both partitions, once");
    assert_eq!(pages, 3);
    // A cursor that is not one is said, and the newest records are shown.
    let (_, bad) = get(&app, "/api/v1/topics/deep/messages?before=zero").await;
    let notes = bad["view"]["notes"].as_array().unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.as_str().unwrap().contains("not a place to read before")),
        "{notes:?}"
    );
    assert_eq!(offsets(&bad).len(), 25);
    w.stop().await;
}
