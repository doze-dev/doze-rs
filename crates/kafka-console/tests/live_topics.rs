//! The Topics pages over a real broker in the same process, as the principal `User:__console`: seeded with topics of several
//! shapes (a compacted one, a gzip batch, headers, a value that is not UTF-8, an open transaction, a topic with a 249-byte
//! name), every route requested through `Router::oneshot`, and the answers compared with the boards on structure (the live
//! render of `tools/board-diff.ts`, written to `target/board-diff-live/`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "live_topics/world.rs"]
mod world;

use axum::http::StatusCode;
use support::{get, send};
use world::*;

#[tokio::test]
async fn every_route_answers_as_the_console_and_has_the_boards_parts() {
    let w = seeded("routes", |_| {}).await;
    let app = w.router();
    let list = page(&app, "/topics", Some("topics-list")).await;
    for name in ["orders", "orders.dlq", "customer.profiles", "gz.events"] {
        assert!(
            list.contains(&format!(">{name}</a>")),
            "{name} in {list:.2000}"
        );
    }
    assert!(
        list.contains("4 topics hold <b>6</b> partitions."),
        "{list:.1500}"
    );
    assert!(
        list.contains("Transaction open") && list.contains("empty: no records yet"),
        "{list:.3000}"
    );
    assert!(list.contains("<span class=\"tag\">compact</span>"));
    page(&app, "/topics/orders?limit=5", Some("topic-detail")).await;
    let detail = page(&app, "/topics/orders", None).await;
    assert!(
        detail.contains("Transaction orders-eos is open."),
        "{detail:.3000}"
    );
    assert!(detail.contains("Above LSO") && detail.contains("Committed"));
    assert!(
        detail.contains("Producers<span class=\"n\">1</span>"),
        "one active producer"
    );
    assert!(
        detail.contains("binary · 0a 09 ff 00 fe 80 2d 38 …"),
        "the binary value"
    );
    page(&app, "/topics/orders.dlq", Some("topic-messages-empty")).await;
    let lso = {
        let p = page(&app, "/topics/orders/partitions", Some("topic-partitions")).await;
        assert!(
            p.contains("held 2") && p.contains("held 1") && p.contains("clear"),
            "{p:.4000}"
        );
        p
    };
    assert!(lso.contains("orders-0") && lso.contains("Delete records"));
    w.stop().await;
}

#[tokio::test]
async fn the_reads_the_records_and_the_filters_answer_as_the_console_and_have_the_boards_parts() {
    let w = seeded("reads", |_| {}).await;
    let app = w.router();
    page(
        &app,
        "/topics/orders/messages?partition=0&offset=4",
        Some("topic-messages-filtered"),
    )
    .await;
    let rc = page(
        &app,
        "/topics/orders/messages?isolation=read_committed",
        Some("topic-readcommitted"),
    )
    .await;
    assert!(
        rc.contains("held back from read_committed consumers"),
        "{rc:.3000}"
    );
    assert!(rc.contains("orders-eos began at offset 4"), "{rc:.5000}");
    let filtered = page(&app, "/topics?q=zzz", Some("topics-filtered")).await;
    assert!(filtered.contains("No topic name contains"));
    // A record of the open transaction and one of the binary value.
    let rec = page(&app, "/topics/orders/messages/0/4", Some("topic-record")).await;
    assert!(
        rec.contains("At or above the last stable offset."),
        "{rec:.3000}"
    );
    assert!(
        rec.contains("orders-eos") && rec.contains("Ongoing") && rec.contains("txn-a"),
        "{rec:.4000}"
    );
    let bin = page(
        &app,
        "/topics/orders/messages/2/0",
        Some("topic-record-binary"),
    )
    .await;
    assert!(
        bin.contains("The value is not text.") && bin.contains("0000  0a 09 ff 00 fe 80 2d 38"),
        "{bin:.3000}"
    );
    assert!(bin.contains("12 bytes · binary"));
    w.stop().await;
}

#[tokio::test]
async fn an_empty_broker_is_the_none_state() {
    let w = world("none", |_| {}).await;
    let app = w.router();
    let body = page(&app, "/topics", Some("topics-none")).await;
    assert!(body.contains("This broker holds no topics"), "{body:.2000}");
    w.stop().await;
}

#[tokio::test]
async fn read_committed_stops_at_the_last_stable_offset_and_read_uncommitted_does_not() {
    let w = seeded("isolation", |_| {}).await;
    let app = w.router();
    let all = page(&app, "/topics/orders/messages?partition=0", None).await;
    let committed = page(
        &app,
        "/topics/orders/messages?partition=0&isolation=read_committed",
        None,
    )
    .await;
    assert_eq!(
        row_count(&all),
        6,
        "four records and the two of the open transaction"
    );
    assert_eq!(row_count(&committed), 4);
    assert_eq!(all.matches("st warn\">Above LSO").count(), 2);
    assert_eq!(committed.matches("st warn\">Above LSO").count(), 0);
    assert!(all.contains("txn-a") && !committed.contains("txn-a"));
    assert!(
        all.contains("2 of them are at or above the last stable offset")
            || all.contains("Newest first"),
        "{all:.2500}"
    );
    w.stop().await;
}

#[tokio::test]
async fn a_compacted_topic_a_gzip_batch_headers_and_a_binary_value_are_shown_as_they_are() {
    let w = seeded("shapes", |_| {}).await;
    let app = w.router();
    let c = page(&app, "/topics/customer.profiles", None).await;
    assert!(
        c.contains("keeps the latest record of each key") && c.contains("compact"),
        "{c:.2500}"
    );
    assert_eq!(row_count(&c), 3);
    let g = page(&app, "/topics/gz.events", None).await;
    assert_eq!(row_count(&g), 3);
    let one = page(&app, "/topics/gz.events/messages/0/1", None).await;
    assert!(
        one.contains("g-1") && one.contains("<dt>Codec</dt><dd>gzip</dd>"),
        "{one:.3000}"
    );
    let h = page(&app, "/topics/orders/messages/0/0", None).await;
    assert!(
        h.contains("content-type") && h.contains("application/json") && h.contains("traceparent"),
        "{h:.3000}"
    );
    assert!(h.contains("<dt>Codec</dt><dd>none</dd>") && h.contains("Position"));
    w.stop().await;
}

#[tokio::test]
async fn the_filters_find_by_key_by_offset_and_by_time_and_a_bad_value_is_a_sentence() {
    let w = seeded("filters", |_| {}).await;
    let app = w.router();
    let by_key = page(&app, "/topics/orders/messages?key=ord-5", None).await;
    assert_eq!(row_count(&by_key), 1, "{by_key:.3000}");
    assert!(by_key.contains("The search found"), "{by_key:.2500}");
    let none = page(&app, "/topics/orders/messages?key=nobody", None).await;
    assert!(
        none.contains("No record matched. The search looked at all"),
        "{none:.2500}"
    );
    let from = page(&app, "/topics/orders/messages?partition=1&offset=1", None).await;
    assert_eq!(
        row_count(&from),
        2 + 1,
        "ord-5, ord-6 and the open transaction's record, from offset 1"
    );
    let ms = now_ms() - 20_000;
    let by_time = page(
        &app,
        &format!("/topics/orders/messages?partition=0&time={ms}"),
        None,
    )
    .await;
    assert!(row_count(&by_time) >= 1, "{by_time:.2500}");
    let bad = page(
        &app,
        "/topics/orders/messages?partition=99&offset=-1&time=yesterday&limit=abc&isolation=x",
        None,
    )
    .await;
    for sentence in [
        "is not an offset",
        "is not a time",
        "is not a number of records",
        "is not an isolation level",
        "has no partition 99",
    ] {
        assert!(bad.contains(sentence), "{sentence}: {bad:.3000}");
    }
    let huge = page(
        &app,
        "/topics/orders/messages?limit=99999999999999999999",
        None,
    )
    .await;
    assert!(huge.contains("is not a number of records"));
    let capped = page(&app, "/topics/orders/messages?limit=100000", None).await;
    assert!(capped.contains("shows at most 200 records"));
    w.stop().await;
}

#[tokio::test]
async fn a_topic_name_of_249_bytes_fits_and_a_longer_one_is_no_topic() {
    let w = world("long", |_| {}).await;
    let name = "n".repeat(249);
    w.create(&name, 1, &[]).await;
    let app = w.router();
    let list = page(&app, "/topics", None).await;
    assert!(list.contains(&format!(">{name}</a>")));
    let t = page(&app, &format!("/topics/{name}"), None).await;
    assert!(t.contains(&format!("<h1 class=\"mono\">{name}</h1>")));
    let (res, _) = send(&app, get(&format!("/topics/{}", "n".repeat(250)))).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let (res, _) = send(&app, get("/topics/no-such-topic")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let (res, _) = send(&app, get("/topics/orders/messages/0/99999999999999999999")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    w.stop().await;
}

#[tokio::test]
async fn a_record_past_the_end_is_a_page_that_says_so_and_not_an_error() {
    let w = seeded("past", |_| {}).await;
    let app = w.router();
    for path in [
        "/topics/orders/messages/0/9999",
        "/topics/orders/messages/9/0",
        "/topics/orders/messages/-1/0",
    ] {
        let (res, body) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}");
        assert!(body.contains("<html"), "{path} is a page in the shell");
    }
    w.stop().await;
}
