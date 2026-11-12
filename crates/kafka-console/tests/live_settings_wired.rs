//! The three sentences of the Settings page that wait on other slices' sources, over a real broker in this process: which
//! version a client speaks (the request recorder), the cleaner's last round (the log facts), and when a group's committed offsets
//! expire (the group's hidden facts). Each says what the broker holds, and nothing where the source has nothing.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_groups;
mod support_live;
mod support_log;
mod support_write;

use axum::http::StatusCode;
use support::get;
use support_live::{recorded, router, traffic};
use support_log::keyed;
use support_write::{page, world};

/// The two sections the test reads, for a failure to show.
fn excerpt(doc: &str) -> String {
    let at = |id: &str| {
        doc.find(id)
            .map_or(String::new(), |i| doc[i..].chars().take(700).collect())
    };
    format!(
        "{}\n---\n{}",
        at("id=\"version-profile\""),
        at("id=\"intervals\"")
    )
}

/// CON-582: the version profile names the clients the recorder heard, with the versions they used; and the cleaner's last
/// round and the expiry of an empty group's offsets are the broker's, not the board's.
#[tokio::test]
async fn settings_names_the_clients_the_last_round_and_the_expiry_from_what_the_broker_holds() {
    let (w, recorder) = recorded("set-wired", false).await;
    traffic(&w).await;
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
    support_groups::commit(&w, "analytics", "orders", &[(0, 5)]).await;
    let app = router(&w, &recorder);
    let (status, doc) = page(&app, get("/settings")).await;
    assert_eq!(status, StatusCode::OK);
    // The clients are the ones that spoke, with the version each used (the larder's traffic: fulfilment fetches at v12; the newest
    // producer is at v9).
    assert!(
        doc.contains("Today <b>fulfilment</b> speaks Fetch v12 and <b>")
            && doc.contains("</b> speaks Produce v9, all inside the ranges."),
        "{}",
        excerpt(&doc)
    );
    assert!(
        doc.contains(", all inside the ranges."),
        "{}",
        excerpt(&doc)
    );
    // The cleaner's last round is the one that ran on the compacted topic, and the planner says what a look now finds.
    assert!(
        doc.contains("Its last round was on <b>compacted.levels</b> at <b>"),
        "{}",
        excerpt(&doc)
    );
    assert!(
        doc.contains("the next look finds it due.")
            || doc.contains("the next look finds nothing due there."),
        "{}",
        excerpt(&doc)
    );
    // The group whose commits the sweep takes first, and in how long.
    assert!(
        doc.contains("kept for 7 d; analytics&#39; expire in 6 d 23 h")
            || doc.contains("kept for 7 d; analytics' expire in 6 d 23 h"),
        "{}",
        excerpt(&doc)
    );
    w.stop().await;
}

/// CON-583: with no recorder no client is named, and with no round put in place the page says so and does not make one up.
#[tokio::test]
async fn without_a_recorder_or_a_round_the_page_names_none() {
    let w = world("set-unwired", |_, _| {}).await;
    let (_, doc) = page(&w.router(), get("/settings")).await;
    assert!(!doc.contains(" speaks "), "no client is named: {doc:.4000}");
    assert!(
        doc.contains("It has put no round in place since the broker started."),
        "{doc:.4000}"
    );
    assert!(!doc.contains("expire in"), "{doc:.4000}");
    w.stop().await;
}
