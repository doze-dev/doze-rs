//! Slice R1b: the JSON API of the Config tab over a real broker in this process: the rows and their words, Check, Save,
//! a list, the two lags, a refused value and Reset, each the IncrementalAlterConfigs the classic pages send.
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

use axum::http::StatusCode;
use support_api::{api, get, post};
use support_write::world;

/// CON-1157, CON-1158, CON-1159: the Config tab lists every config with its words; Check changes nothing and Save does;
/// a list appends; the two lags go together and min above max is refused with what would pass; a value Kafka will not take
/// is INVALID_CONFIG in its words; Reset deletes the topic's value.
#[tokio::test]
async fn the_config_tab_edits_checks_saves_appends_and_resets() {
    let w = world("api-config", |_, _| {}).await;
    w.create_topic("t", 1, &[("retention.ms", "3600000")]).await;
    let app = api(&w);
    let (s, v) = get(&app, "/api/v1/topics/t/config").await;
    assert_eq!(s, StatusCode::OK);
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 33);
    let ret = rows.iter().find(|r| r["name"] == "retention.ms").unwrap();
    assert_eq!(ret["source"], "dynamic_topic");
    assert_eq!(ret["changed"], true);
    assert!(ret["effect"].as_str().unwrap().contains("retention pass"));
    assert!(ret["takes_over"].as_str().unwrap().contains("takes over"));
    assert_eq!(ret["places"].as_array().unwrap().len(), 5);
    let check = [
        ("key", "retention.ms"),
        ("op", "set"),
        ("value", "7200000"),
        ("check", "1"),
    ];
    let (s, v) = post(&app, "/api/v1/topics/t/config/edit", &check).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["kind"], "checked");
    assert_eq!(
        w.config("t", "retention.ms").await.as_deref(),
        Some("3600000")
    );
    let (s, v) = post(&app, "/api/v1/topics/t/config/edit", &check[..3]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["kind"], "saved");
    assert_eq!(
        w.config("t", "retention.ms").await.as_deref(),
        Some("7200000")
    );
    let (s, v) = post(
        &app,
        "/api/v1/topics/t/config/edit",
        &[
            ("key", "cleanup.policy"),
            ("op", "append"),
            ("value", "compact"),
        ],
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        w.config("t", "cleanup.policy").await.as_deref(),
        Some("delete,compact")
    );
    let lags = [
        ("key", "min.compaction.lag.ms"),
        ("value", "50000"),
        ("other", "10000"),
    ];
    let (s, v) = post(&app, "/api/v1/topics/t/config/edit", &lags).await;
    assert_eq!(
        (s, v["outcome"].as_str()),
        (StatusCode::OK, Some("refused")),
        "{v}"
    );
    assert_eq!(v["code"], "INVALID_CONFIG");
    assert_eq!(v["detail"]["refused"]["kind"], "lags");
    assert_eq!(v["message"], v["detail"]["refused"]["refusal"]["message"]);
    let (s, v) = post(
        &app,
        "/api/v1/topics/t/config/edit",
        &[("key", "min.cleanable.dirty.ratio"), ("value", "7")],
    )
    .await;
    assert_eq!(
        (s, v["outcome"].as_str()),
        (StatusCode::OK, Some("refused")),
        "{v}"
    );
    assert_eq!(v["detail"]["refused"]["refusal"]["code"], 40);
    let (_, v) = get(&app, "/api/v1/topics/t/config/reset?key=retention.ms").await;
    // What takes over is the broker's: a flag or Kafka's default, not the topic's value.
    let after = v["view"]["after"]["value"].as_str().unwrap().to_owned();
    assert_ne!(after, "7200000");
    let (s, v) = post(
        &app,
        "/api/v1/topics/t/config/reset",
        &[("key", "retention.ms")],
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v) = get(&app, "/api/v1/topics/t/config").await;
    let ret = v["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "retention.ms")
        .unwrap()
        .clone();
    assert_ne!(ret["source"], "dynamic_topic");
    assert_eq!(ret["value"], after.as_str());
    let (s, v) = get(&app, "/api/v1/topics/t/config/edit?key=no.such.key").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["missing"]["code"], "NOT_FOUND");
    w.stop().await;
}

/// J124: resetting a key that decides when a segment rolls says what that does to the log there is: the sealed segments keep
/// their size, the active one of each partition rolls by the value after; the other keys say nothing of segments.
#[tokio::test]
async fn a_reset_of_segment_bytes_says_which_segments_keep_their_size() {
    let w = world("api-config-segments", |_, _| {}).await;
    w.create_topic("t", 2, &[("segment.bytes", "1048576")])
        .await;
    let app = api(&w);
    let (_, v) = get(&app, "/api/v1/topics/t/config/reset?key=segment.bytes").await;
    assert_eq!(v["view"]["segments"]["partitions"], 2, "{v}");
    assert_eq!(v["view"]["segments"]["sealed"], 0);
    let note = v["segments_note"].as_str().unwrap();
    assert!(
        note.starts_with(
            "No segment is sealed yet. The active segment of each of the 2 partitions"
        ),
        "{note}"
    );
    assert!(
        note.ends_with("rolls once it passes the value after."),
        "{note}"
    );
    let (_, v) = get(&app, "/api/v1/topics/t/config/reset?key=retention.ms").await;
    assert!(
        v["segments_note"].is_null() && v["view"]["segments"].is_null(),
        "{v}"
    );
    w.stop().await;
}

/// J121: a save of a key the cleaner reads says what its planner makes of the log now, under the new value, in the Log tab's
/// words; a key it does not read says nothing of the cleaner.
#[tokio::test]
async fn a_saved_cleaner_key_says_what_the_planner_makes_of_the_log_now() {
    let w = world("api-config-cleaner", |_, _| {}).await;
    w.create_topic("c", 1, &[("cleanup.policy", "compact")])
        .await;
    w.fill("c", 10, 3).await;
    let app = api(&w);
    let (s, v) = post(
        &app,
        "/api/v1/topics/c/config/edit",
        &[
            ("key", "min.cleanable.dirty.ratio"),
            ("op", "set"),
            ("value", "0.9"),
        ],
    )
    .await;
    assert_eq!(
        (s, v["outcome"].as_str()),
        (StatusCode::OK, Some("done")),
        "{v}"
    );
    let now = v["answer"]["cleaner_now"]
        .as_str()
        .unwrap_or_else(|| panic!("{v}"));
    assert_eq!(now, "1 segment, nothing dirty: not due");
    let (_, v) = post(
        &app,
        "/api/v1/topics/c/config/edit",
        &[("key", "retention.ms"), ("op", "set"), ("value", "3600000")],
    )
    .await;
    assert!(v["answer"]["cleaner_now"].is_null(), "{v}");
    w.stop().await;
}
