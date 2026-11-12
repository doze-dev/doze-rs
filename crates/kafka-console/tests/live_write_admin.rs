//! Add partitions, delete records and delete topic over a real broker in this process; a GET never writes; and the pages
//! when the caller is not a super user (what a client would be told on a broker with ACLs).
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_write;

use axum::http::{StatusCode, header};
use support::get;
use support_write::{World, page, post_form, world};

async fn produce_many(w: &World, topic: &str, n: usize) {
    let app = w.router();
    for i in 0..n {
        let value = format!("v{i}");
        let (status, doc) = page(
            &app,
            post_form(
                &format!("/topics/{topic}/produce"),
                &[
                    ("partition", "0"),
                    ("key", "k"),
                    ("value", &value),
                    ("acks", "1"),
                ],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(doc.contains(&format!("at offset {i}.")), "{doc:.300}");
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn partitions_can_be_added_and_never_taken_away() {
    let w = world("partitions", |_, _| {}).await;
    w.create_topic("orders", 3, &[]).await;
    let app = w.router();
    let (status, dialog) = page(&app, get("/topics/orders/partitions/add")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        dialog.contains("Add partitions to <span class=\"mono\">orders</span>")
            && dialog.contains("now 3")
            && dialog.contains("value=\"4\""),
        "{dialog:.400}"
    );
    assert!(
        dialog.contains("4 to 1,000"),
        "the limit is the broker's own: {dialog:.2000}"
    );
    let (res, _) = support::send(
        &app,
        post_form("/topics/orders/partitions/add", &[("count", "5")]),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(res.headers()[header::LOCATION], "/topics/orders/partitions");
    assert_eq!(w.topics().await, [("orders".to_owned(), 5)]);
    for (count, words) in [
        (
            "2",
            "The topic orders currently has 5 partition(s); 2 would not be an increase.",
        ),
        ("5", "Topic already has 5 partition(s)."),
        (
            "1001",
            "1001 partitions is above this broker's limit of 1000 (--max-partitions)",
        ),
    ] {
        let (status, doc) = page(
            &app,
            post_form("/topics/orders/partitions/add", &[("count", count)]),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{count}");
        assert!(
            doc.contains("Refused · INVALID_PARTITIONS (37)"),
            "{count}: {doc:.1500}"
        );
        assert!(
            doc.contains(words),
            "{count}: {words:?} is not in {doc:.2500}"
        );
        assert!(
            doc.contains("What would pass") && doc.contains("any total from 6 to 1,000."),
            "{count}"
        );
    }
    assert_eq!(
        w.topics().await,
        [("orders".to_owned(), 5)],
        "no refusal changed anything"
    );
    let (status, nan) = page(
        &app,
        post_form("/topics/orders/partitions/add", &[("count", "lots")]),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(nan.contains("A whole number is needed, such as 3."));
    // A script is answered the dialog alone.
    let (_, alone) = page(
        &app,
        support_write::fragment_get("/topics/orders/partitions/add"),
    )
    .await;
    assert!(
        alone.starts_with("<div class=\"modal\" id=\"add-partitions\" role=\"dialog\""),
        "{alone:.200}"
    );
    assert!(!alone.contains("<html"));
    w.stop().await;
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn records_are_deleted_up_to_an_offset_and_the_log_start_is_that_offset() {
    let w = world("records", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    w.create_topic("inventory.levels", 1, &[("cleanup.policy", "compact")])
        .await;
    produce_many(&w, "orders", 10).await;
    let app = w.router();
    let (_, dialog) = page(
        &app,
        get("/topics/orders/delete-records?partition=0&offset=7"),
    )
    .await;
    assert!(
        dialog.contains("log start now 0 · end 10")
            && dialog.contains("This removes 7 records")
            && dialog.contains("Delete 7 records"),
        "{dialog:.600}"
    );
    let (res, _) = support::send(
        &app,
        post_form(
            "/topics/orders/delete-records",
            &[("partition", "0"), ("offset", "7")],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        w.offset("orders", 0, -2).await,
        7,
        "the earliest offset is the one deleted up to"
    );
    assert_eq!(w.offset("orders", 0, -1).await, 10);
    // Below the start changes nothing, past the end is refused in Kafka's code, and -1 is everything.
    let (_, past) = page(
        &app,
        post_form(
            "/topics/orders/delete-records",
            &[("partition", "0"), ("offset", "11")],
        ),
    )
    .await;
    assert!(
        past.contains("Refused · OFFSET_OUT_OF_RANGE (1)")
            && past.contains("-1, or an offset from 0 to 10."),
        "{past:.1500}"
    );
    assert_eq!(w.offset("orders", 0, -2).await, 7);
    let (res, _) = support::send(
        &app,
        post_form(
            "/topics/orders/delete-records",
            &[("partition", "0"), ("offset", "3")],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        w.offset("orders", 0, -2).await,
        7,
        "an offset below the start changes nothing"
    );
    let (res, _) = support::send(
        &app,
        post_form(
            "/topics/orders/delete-records",
            &[("partition", "0"), ("offset", "-1")],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(w.offset("orders", 0, -2).await, 10);
    // A topic that only compacts keeps every key's latest record: refused with Kafka's description of the code.
    let (status, refused) = page(
        &app,
        post_form(
            "/topics/inventory.levels/delete-records",
            &[("partition", "0"), ("offset", "0")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        refused.contains("Refused · POLICY_VIOLATION (44)")
            && refused.contains("Request parameters do not satisfy the configured policy."),
        "{refused:.1500}"
    );
    assert!(
        refused.contains("What removes records here instead") && refused.contains("delete,compact")
    );
    // A partition the topic does not have is not a dialog.
    let (status, _) = page(&app, get("/topics/orders/delete-records?partition=4")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    w.stop().await;
}

#[tokio::test]
async fn a_topic_is_deleted_only_when_its_name_is_typed_and_is_then_gone() {
    let w = world("delete", |_, _| {}).await;
    w.create_topic("orders", 3, &[]).await;
    w.create_topic("orders.dlq", 1, &[]).await;
    produce_many(&w, "orders", 4).await;
    let app = w.router();
    let (_, dialog) = page(&app, get("/topics/orders/delete")).await;
    assert!(
        dialog.contains("Delete <span class=\"mono\">orders</span>?")
            && dialog.contains("<b>4 records</b> in 3 partitions"),
        "{dialog:.600}"
    );
    assert!(!dialog.contains("It is in use."), "nothing reads it");
    let (status, wrong) = page(
        &app,
        post_form("/topics/orders/delete", &[("confirm", "order")]),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(wrong.contains("Type the name of the topic to confirm."));
    assert_eq!(w.topics().await.len(), 2);
    let (status, gone) = page(
        &app,
        post_form("/topics/orders/delete", &[("confirm", "orders")]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        gone.contains("Deleted orders.")
            && gone.contains("4 records and 3 partitions are gone from disk"),
        "{gone:.600}"
    );
    assert!(
        gone.contains("1 topic, 1 partition.")
            && gone.contains("orders.dlq")
            && gone.contains("empty: no records yet"),
        "{gone:.1500}"
    );
    assert_eq!(w.topics().await, [("orders.dlq".to_owned(), 1)]);
    // A second time it is not there, and the page says so.
    let (status, none) = page(
        &app,
        post_form("/topics/orders/delete", &[("confirm", "orders")]),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(none.contains("There is no topic called"));
    w.stop().await;
}
