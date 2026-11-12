//! The recorder and Live over a real broker: real client traffic over the Kafka listener (a raw client that speaks the wire), the
//! tape that results, its drawers, the clients, the docks and the first-request page, as the console principal. The live render
//! of the boards is written to `target/board-diff-live/`.
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

use axum::http::StatusCode;
use kafka_console::base::Base;
use kafka_console::model::WireQuery;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_wire::messages::*;
use support::get;
use support_write::{page, world};
use tokio_util::sync::CancellationToken;

/// A client that speaks the Kafka wire over a real connection.
use support_live::*;

#[tokio::test]
async fn the_tape_shows_what_the_clients_did_and_what_was_refused() {
    let (w, rec) = recorded("tape", false).await;
    traffic(&w).await;
    let app = router(&w, &rec);
    let (status, live) = page(&app, get("/live?idle=0")).await;
    assert_eq!(status, StatusCode::OK);
    for want in [
        "INVALID_RECORD (87)",
        "MESSAGE_TOO_LARGE (10)",
        "Refused",
        "fulfilment",
        "Produce",
        "Metadata",
        "orders-0",
        "× 25",
        "1 record",
        "caught up, no records",
        "Refused among the rows kept:",
        "data-extra=\"live-sentence\"",
        "requests in the last minute.",
    ] {
        assert!(live.contains(want), "{want:?} in {live:.6000}");
    }
    assert!(
        live.contains("INVALID_PRODUCER_EPOCH (47)") || live.contains("PRODUCER_FENCED (90)"),
        "the fenced producer"
    );
    // A hostile client name and software are text.
    assert!(
        !live.contains("<script>alert(1)") && !live.contains("<img src=x"),
        "{live:.4000}"
    );
    assert!(live.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    // Idle polls are hidden by default and the fold is one row.
    let (_, quiet) = page(&app, get("/live")).await;
    assert!(!quiet.contains("× 25") && quiet.contains("idle polls hidden"));
    w.stop().await;
}

#[tokio::test]
async fn each_refusal_opens_its_own_drawer_with_kafkas_words_and_the_rule() {
    let (w, rec) = recorded("drawers", false).await;
    traffic(&w).await;
    let app = router(&w, &rec);
    let tape = rec.tape(
        &WireQuery {
            hide_idle: false,
            limit: 100,
            ..WireQuery::default()
        },
        kafka_console::source::now_ms(),
    );
    let id_of = |code: i16| {
        tape.rows
            .iter()
            .find(|r| r.code == code)
            .map(|r| r.id)
            .unwrap_or_else(|| panic!("no row with code {code}"))
    };
    let drawer = |id: u64| {
        let app = app.clone();
        async move {
            let (status, d) = page(&app, get(&format!("/live/{id}"))).await;
            assert_eq!(status, StatusCode::OK, "{d:.500}");
            d
        }
    };
    let refused = drawer(id_of(87)).await;
    for want in [
        "INVALID_RECORD (87)",
        "Kafka's words",
        "The rule",
        "What would pass",
        "cleanup.policy=compact",
        "kcat -P -K:",
        "Written",
        "Nothing",
        "Produce with a key",
    ] {
        assert!(refused.contains(want), "{want:?} in {refused:.5000}");
    }
    let large = drawer(id_of(10)).await;
    for want in [
        "MESSAGE_TOO_LARGE (10)",
        "Batch size is within the limit",
        "1,048,588",
        "the broker value applies",
        "Raise max.message.bytes",
    ] {
        assert!(large.contains(want), "{want:?} in {large:.5000}");
    }
    let fenced_code = tape
        .rows
        .iter()
        .find(|r| matches!(r.code, 47 | 90))
        .map(|r| r.code)
        .expect("a fenced row");
    let fenced = drawer(id_of(fenced_code)).await;
    for want in [
        "How it came to this",
        "InitProducerId",
        "What would pass",
        "orders-eos",
    ] {
        assert!(fenced.contains(want), "{want:?} in {fenced:.5000}");
    }
    let done = tape.rows.iter().find(|r| r.api == "Fetch").unwrap().id;
    let detail = drawer(done).await;
    for want in [
        "Fetch v12",
        "What the broker kept.",
        "Copy as",
        "kcat -C",
        "Correlation id",
        "confluent-kafka-python",
    ] {
        assert!(detail.contains(want), "{want:?} in {detail:.5000}");
    }
    let (status, gone) = page(&app, get("/live/999999")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(gone.contains("no longer on the wire"));
    let (_, clients) = page(&app, get("/live/clients")).await;
    for want in [
        "fulfilment",
        "admin-cli",
        "confluent-kafka-python",
        "Only clients that are connected now.",
    ] {
        assert!(clients.contains(want), "{want:?} in {clients:.3000}");
    }
    assert!(!clients.contains("<script>alert(1)"));
    w.stop().await;
}

#[tokio::test]
async fn the_docks_the_first_request_and_home_come_from_the_same_tape() {
    let (w, rec) = recorded("docks", false).await;
    traffic(&w).await;
    let app = router(&w, &rec);
    let (status, topic) = page(&app, get("/topics/orders/wire")).await;
    assert_eq!(status, StatusCode::OK);
    for want in [
        "On the wire: orders",
        "Collapse to the dock",
        "orders-0",
        "Topic and partition",
    ] {
        assert!(topic.contains(want), "{want:?} in {topic:.3000}");
    }
    let (_, group_page) = page(&app, get("/groups/fulfilment/wire")).await;
    assert!(
        group_page.contains("Every client request that touched fulfilment"),
        "{group_page:.1500}"
    );
    let (_, first) = page(&app, get("/connect/first-request")).await;
    assert!(
        first.contains("A new client just connected")
            && first.contains("sent its first request at"),
        "{first:.2500}"
    );
    w.stop().await;
}

#[tokio::test]
async fn an_acl_refusal_is_amber_denied_and_not_violet() {
    let (w, rec) = recorded("denied", true).await;
    w.create_topic("orders", 1, &[]).await;
    let mut c = Wire::connect(&w).await;
    c.call(ApiKey::ApiVersions, 3, "intruder", &versions("kcat"))
        .await;
    c.call(
        ApiKey::Produce,
        9,
        "intruder",
        &produce("orders", batch(Some("k"), b"v".to_vec(), None), None),
    )
    .await;
    let app = router(&w, &rec);
    let (_, live) = page(&app, get("/live")).await;
    assert!(
        live.contains("TOPIC_AUTHORIZATION_FAILED (29)"),
        "{live:.4000}"
    );
    assert!(
        live.contains("<span class=\"st warn\">Denied</span>"),
        "amber, with the word"
    );
    assert!(!live.contains("st refused\">Denied"));
    let id = rec
        .tape(&WireQuery::default(), kafka_console::source::now_ms())
        .rows
        .iter()
        .find(|r| r.code == 29)
        .unwrap()
        .id;
    let (_, d) = page(&app, get(&format!("/live/{id}"))).await;
    for want in [
        "TOPIC_AUTHORIZATION_FAILED (29)",
        "An ACL allows this",
        "Add an ACL for the principal on the topic",
        "callout warn",
    ] {
        assert!(d.contains(want), "{want:?} in {d:.4000}");
    }
    w.stop().await;
}

#[tokio::test]
async fn a_console_without_a_recorder_says_the_wire_is_empty_and_the_broker_without_an_observer_is_unchanged()
 {
    // No observer installed: the broker serves, nothing is recorded, the page is honest.
    let w = world("none", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    let mut c = Wire::connect(&w).await;
    let answer = c
        .call(ApiKey::ApiVersions, 3, "app", &versions("sarama"))
        .await;
    assert!(answer.len() > 20, "the broker answers as it always did");
    let live = Live::new(w.backend.clone(), w.facts.clone());
    let state = State::new(
        Source::Live(Box::new(live)),
        Base::root(),
        vec![],
        CancellationToken::new(),
    );
    let app = kafka_console::app::router(state);
    let (status, live) = page(&app, get("/live")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        live.contains("Nothing has reached the broker"),
        "{live:.1500}"
    );
    w.stop().await;
}
