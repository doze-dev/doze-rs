//! The Settings answer's three facts that wait on other sources (the clients the recorder heard, the cleaner's last round, the
//! soonest expiry of a group's offsets), over a real broker in this process: with no source it names none and invents none
//! (CON-583), and with one it names what the broker holds.
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
mod support_live;
mod support_log;
mod support_write;

use axum::Router;
use axum::http::StatusCode;
use serde_json::Value;
use support_api::get;
use support_log::keyed;

async fn settings(app: &Router) -> Value {
    let (s, v) = get(app, "/api/v1/settings").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v["settings"].clone()
}

/// The note of the Offsets retention interval.
fn offsets_note(v: &Value) -> String {
    v["intervals"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["note"].as_str().unwrap().starts_with("kept for"))
        .unwrap_or_else(|| panic!("no offsets retention row: {}", v["intervals"]))["note"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// CON-583: without a recorder no client is named, with no round put in place none is made up, and with no group's commit no
/// expiry is named; then, given each, the answer names it (so the empty answer is the sources', not a page that never fills).
#[tokio::test]
async fn without_a_recorder_or_a_round_the_settings_answer_names_none() {
    let w = support_write::world("as-unwired", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    let v = settings(&support_api::api(&w)).await;
    assert_eq!(
        v["profile"]["speaking"],
        serde_json::json!([]),
        "no client is named"
    );
    assert!(
        v["intervals"]["round"].is_null(),
        "no round: {}",
        v["intervals"]
    );
    assert_eq!(offsets_note(&v), "kept for 7 d", "no group is named");
    w.stop().await;

    // The same broker with a recorder, a round of the cleaner and a commit: each is named.
    let (w, recorder) = support_live::recorded("as-wired", false).await;
    support_live::traffic(&w).await;
    w.create_topic(
        "compacted.levels",
        1,
        &[
            ("cleanup.policy", "compact"),
            ("segment.bytes", "1024"),
            ("min.cleanable.dirty.ratio", "0.01"),
        ],
    )
    .await;
    for _ in 0..2 {
        assert_eq!(
            w.produce_batch("compacted.levels", 0, keyed(&[("k", Some("v"))]), None)
                .await
                .0,
            0
        );
    }
    w.fill("compacted.levels", 60, 5).await;
    w.broker.check_cleaner().await;
    support_groups::commit(&w, "analytics", "orders", &[(0, 0)]).await;
    let v = settings(&support_live::router(&w, &recorder)).await;
    let speaking = v["profile"]["speaking"].as_array().unwrap();
    assert!(
        speaking
            .iter()
            .any(|s| s["client"] == "fulfilment" && s["api"] == "Fetch" && s["version"] == 12),
        "{speaking:?}"
    );
    assert_eq!(
        v["intervals"]["round"]["topic"], "compacted.levels",
        "{}",
        v["intervals"]
    );
    // Read in the same millisecond as the commit (or with the commit's clock a hair ahead), the whole 7 d is left; any
    // later it is 6 d 23 h. A fast runner answered "7 d" once (app-land, run 43622272665): both are right.
    let note = offsets_note(&v);
    assert!(
        note == "kept for 7 d; analytics' expire in 7 d"
            || note.starts_with("kept for 7 d; analytics' expire in 6 d 23 h"),
        "{}",
        note
    );
    w.stop().await;
}
