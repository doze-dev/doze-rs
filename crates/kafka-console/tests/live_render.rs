//! The live render of the twelve boards over a real broker with real client traffic: written to `target/board-diff-live/`.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_live;
mod support_write;

use axum::Router;
use axum::http::StatusCode;
use kafka_console::model::WireQuery;
use kafka_wire::messages::*;
use support::get;
use support_live::*;
use support_write::page;

fn live_dir() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), std::path::PathBuf::from);
    target.join("board-diff-live")
}

/// A page, asserted a 200, written for the live board-diff under the board's id.
async fn board(app: &Router, path: &str, id: &str) -> String {
    let (status, html) = page(app, get(path)).await;
    assert_eq!(status, StatusCode::OK, "{path}: {html:.400}");
    let dir = live_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{id}.html")), &html).unwrap();
    html
}

#[tokio::test]
async fn the_live_render_of_the_boards_over_a_real_broker() {
    // Quiet: nothing has happened yet.
    let (w, rec) = recorded("render-quiet", false).await;
    let quiet = board(&router(&w, &rec), "/live", "live-quiet").await;
    assert!(quiet.contains("No requests yet"));
    w.stop().await;
    // The tape, its drawers, the clients, the docks and the first request.
    let (w, rec) = recorded("render", false).await;
    traffic(&w).await;
    let mut g = Wire::connect(&w).await;
    let mut join = JoinGroupRequest::default();
    join.group_id = GroupId(s("fulfilment"));
    join.session_timeout_ms = 10_000;
    join.rebalance_timeout_ms = 10_000;
    join.protocol_type = s("consumer");
    g.call(ApiKey::JoinGroup, 5, "fulfilment", &join).await;
    let app = router(&w, &rec);
    let wire = board(&app, "/live", "live-wire").await;
    assert!(wire.contains("Refusals"));
    let tape = rec.tape(
        &WireQuery {
            hide_idle: false,
            limit: 100,
            ..WireQuery::default()
        },
        kafka_console::source::now_ms(),
    );
    let id = |f: &dyn Fn(&kafka_console::model::WireRow) -> bool| {
        tape.rows.iter().find(|r| f(r)).unwrap().id
    };
    board(
        &app,
        &format!("/live?until={}", id(&|r| r.api == "Metadata")),
        "live-paused",
    )
    .await;
    board(
        &app,
        &format!("/live/{}", id(&|r| r.api == "Fetch")),
        "request-detail",
    )
    .await;
    board(
        &app,
        &format!("/live/{}", id(&|r| r.code == 87)),
        "request-refused",
    )
    .await;
    board(
        &app,
        &format!("/live/{}", id(&|r| r.code == 10)),
        "request-too-large",
    )
    .await;
    board(
        &app,
        &format!("/live/{}", id(&|r| matches!(r.code, 47 | 90))),
        "request-fenced",
    )
    .await;
    board(&app, "/live/clients", "live-clients").await;
    board(&app, "/connect/first-request", "connect-first-request").await;
    board(&app, "/topics/orders/wire", "topic-wire").await;
    board(&app, "/groups/fulfilment/wire", "group-wire").await;
    w.stop().await;
    // A busy minute: a stream of produces folds into one row.
    let (w, rec) = recorded("render-busy", false).await;
    w.create_topic("clickstream", 1, &[]).await;
    let mut c = Wire::connect(&w).await;
    c.call(
        ApiKey::ApiVersions,
        3,
        "notifications",
        &versions("kafkajs"),
    )
    .await;
    for i in 0..150 {
        c.call(
            ApiKey::Produce,
            9,
            "notifications",
            &produce(
                "clickstream",
                batch(Some("k"), format!("v{i}").into_bytes(), None),
                None,
            ),
        )
        .await;
    }
    let busy = board(&router(&w, &rec), "/live", "live-load").await;
    assert!(busy.contains("× 150"), "{busy:.3000}");
    w.stop().await;
}
