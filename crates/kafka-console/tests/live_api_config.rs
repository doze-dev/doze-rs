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
        (s, v["refused"].as_bool(), v["status"].as_u64()),
        (StatusCode::OK, Some(true), Some(422)),
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
        (s, v["refused"].as_bool(), v["status"].as_u64()),
        (StatusCode::OK, Some(true), Some(422)),
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
    let (s, _) = get(&app, "/api/v1/topics/t/config/edit?key=no.such.key").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    w.stop().await;
}
