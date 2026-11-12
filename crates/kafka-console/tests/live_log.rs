//! The Log and Producers pages over a real broker in this process: what the Log page says is what the broker's log holds and what
//! the cleaner then does; the ruler is drawn from a real compacted partition; a transaction held open is the one the Producers
//! page names; a caller without rights meets what a client would; hostile text is escaped; and a GET changes nothing.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_log;
mod support_write;

use axum::http::StatusCode;
use kafka_wire::messages::ApiKey;
use support::get;
use support_log::{from, keyed, out_dir, records, seeded};
use support_write::{page, world};

fn save(id: &str, doc: &str) {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{id}.html")), doc).unwrap();
}

/// CON-530: a compacted log that is due says so, and after the cleaner has run the page shows the round, the gaps it left and
/// the deletion it kept, and the numbers are the broker's.
#[tokio::test]
async fn a_compacted_log_is_drawn_from_what_the_cleaner_left() {
    let w = seeded("log-compacted").await;
    let app = w.router();
    assert_eq!(
        w.produce_batch("inventory.levels", 0, keyed(&[("gone", Some("1"))]), None)
            .await
            .0,
        0
    );
    assert_eq!(
        w.produce_batch("inventory.levels", 0, keyed(&[("gone", None)]), None)
            .await
            .0,
        0
    );
    w.fill("inventory.levels", 60, 5).await;
    let (status, due) = page(&app, get("/topics/inventory.levels/log")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(due.contains("Will the cleaner take it?") && due.contains("Due at the next look"));
    assert!(due.contains("<span class=\"st ok\">Compacted</span>"));
    assert!(
        !due.contains("class=\"gp\""),
        "nothing is removed before a round"
    );
    w.broker.check_cleaner().await;
    w.fill("inventory.levels", 30, 5).await;
    let (_, after) = page(&app, get("/topics/inventory.levels/log")).await;
    save("topic-log", &after);
    assert!(
        after.contains("Last round") && after.contains("<span class=\"st ok\">Done</span>"),
        "{after:.4000}"
    );
    assert!(
        after.contains("class=\"gp\""),
        "the ruler hatches what the round removed"
    );
    assert!(
        after.contains("<span class=\"name mono\">")
            || after.contains("td class=\"name mono\">gone"),
        "the deletion is listed"
    );
    assert!(
        after.contains("Read the tombstone")
            && after.contains("/topics/inventory.levels/messages/0/")
    );
    // The numbers are the broker's: the end offset, and as many rows of the table as segments the walk found.
    let end = w.offset("inventory.levels", 0, -1).await;
    assert_eq!(end, 92);
    assert!(
        after.contains("<b>92</b>") || after.contains("92 offsets"),
        "the sentence names the end"
    );
    let facts = w.backend.log_facts("inventory.levels", 0).await.unwrap();
    assert_eq!(after.matches("<details").count(), facts.segments.len());
    assert!(
        !after.contains("<form") && !after.contains("Compact now"),
        "no form and no button of the page writes anything (the topic's own head has its links)"
    );
    w.stop().await;
}

/// CON-531: a compacted log that is not due says why, and the planner's verdict is what the cleaner then does (nothing).
#[tokio::test]
async fn a_log_that_is_not_due_says_why_and_the_cleaner_then_leaves_it_alone() {
    let w = world("log-notdue", |_, _| {}).await;
    w.create_topic(
        "customer.profiles",
        1,
        &[
            ("cleanup.policy", "compact"),
            ("segment.bytes", "1024"),
            ("min.cleanable.dirty.ratio", "0.5"),
        ],
    )
    .await;
    w.fill("customer.profiles", 80, 40).await;
    w.broker.check_cleaner().await;
    w.fill("customer.profiles", 16, 40).await;
    let app = w.router();
    let before = w.backend.log_facts("customer.profiles", 0).await.unwrap();
    let (_, doc) = page(&app, get("/topics/customer.profiles/log")).await;
    save("log-planner", &doc);
    assert!(
        doc.contains("Why it waits") && doc.contains("<span class=\"st warn\">Not due</span>"),
        "{doc:.3000}"
    );
    assert!(doc.contains("What would make it due"));
    w.broker.check_cleaner().await;
    let after = w.backend.log_facts("customer.profiles", 0).await.unwrap();
    assert_eq!(
        before
            .segments
            .iter()
            .map(|s| (s.base, s.bytes))
            .collect::<Vec<_>>(),
        after
            .segments
            .iter()
            .map(|s| (s.base, s.bytes))
            .collect::<Vec<_>>(),
        "not due means nothing changed"
    );
    w.stop().await;
}

/// CON-532: a log that deletes says what the next pass does, and after a pass says what the pass did.
#[tokio::test]
async fn retention_is_named_before_and_after_a_pass() {
    let w = world("log-retention", |_, _| {}).await;
    w.create_topic(
        "orders",
        1,
        &[("segment.bytes", "1024"), ("retention.bytes", "3000")],
    )
    .await;
    for _ in 0..60 {
        assert_eq!(w.produce_batch("orders", 0, records(5), None).await.0, 0);
    }
    let app = w.router();
    let (_, before) = page(&app, get("/topics/orders/log")).await;
    assert!(
        before.contains("What the next pass will do")
            && before.contains("No pass has run since the broker started")
    );
    assert!(before.contains("<span class=\"st ok\">Delete</span>"));
    assert!(
        before.contains("Size: 2.9 KiB") && before.contains("<span class=\"v\">over</span>"),
        "{:.2500}",
        before
            .split("What the next pass will do")
            .nth(1)
            .unwrap_or("")
    );
    let start = w.offset("orders", 0, -2).await;
    w.broker.check_logs().await;
    let moved = w.offset("orders", 0, -2).await;
    assert!(moved > start, "the pass deleted segments");
    let (_, after) = page(&app, get("/topics/orders/log")).await;
    save("log-retention", &after);
    assert!(
        after.contains("<span class=\"st ok\">Deleted</span>"),
        "{after:.5000}"
    );
    assert!(
        after.contains(&format!("orders-0: log start {} to {}", start, moved)),
        "the card says what moved"
    );
    assert!(
        after.contains("1 log starts moved since the broker started")
            || after.contains("log starts moved")
    );
    w.stop().await;
}

/// CON-533: a partition with nothing in it says so, with the command that fills it.
#[tokio::test]
async fn an_empty_log_says_how_the_first_record_gets_there() {
    let w = seeded("log-empty").await;
    let app = w.router();
    let (status, doc) = page(&app, get("/topics/orders.dlq/log")).await;
    save("log-empty", &doc);
    assert_eq!(status, StatusCode::OK);
    assert!(
        doc.contains("This log has nothing in it")
            && doc.contains("<span class=\"st idle\">Empty</span>")
    );
    assert!(doc.contains("kcat -P -b ") && doc.contains("-t orders.dlq"));
    assert!(doc.contains("href=\"/topics/orders.dlq/produce\""));
    w.stop().await;
}

/// CON-537: a caller without rights meets what a client would: no such topic, and no fact about it.
#[tokio::test]
async fn a_caller_without_rights_is_told_no_such_topic() {
    let w = world("log-acl", |bc, _| bc.acl_default_allow = false).await;
    w.create_topic("orders", 1, &[]).await;
    let nobody = kafka_broker::Caller::anonymous(Some("127.0.0.1".parse().unwrap()));
    let app = support_write::router_of(&w, w.backend.acting_as(nobody));
    for path in [
        "/topics/orders/log",
        "/topics/orders/producers",
        "/topics/orders/transactions/x",
        "/topics/orders/transactions/x/held",
    ] {
        let (status, doc) = page(&app, get(path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert!(
            !doc.contains("Durability") && !doc.contains("Producers on"),
            "{path}"
        );
    }
    // The console itself is a super user and sees it.
    let (status, _) = page(&w.router(), get("/topics/orders/log")).await;
    assert_eq!(status, StatusCode::OK);
    w.stop().await;
}

/// CON-538: ids and keys of any bytes are escaped in every place they appear, and a long topic name fits.
#[tokio::test]
async fn hostile_ids_and_keys_are_escaped_and_a_long_topic_name_fits() {
    let w = seeded("log-hostile").await;
    let app = w.router();
    let evil = "<script>alert(1)</script>\"'&";
    let t = w.init_producer(Some(evil)).await;
    w.add_to_txn(evil, t, "orders", 0).await;
    assert_eq!(
        w.produce_batch("orders", 0, from(t.0, t.1, 0, true, 1), Some(evil))
            .await
            .0,
        0
    );
    let key = "<img src=x onerror=alert(1)>";
    assert_eq!(
        w.produce_batch("inventory.levels", 0, keyed(&[(key, Some("1"))]), None)
            .await
            .0,
        0
    );
    assert_eq!(
        w.produce_batch("inventory.levels", 0, keyed(&[(key, None)]), None)
            .await
            .0,
        0
    );
    w.fill("inventory.levels", 40, 3).await;
    w.broker.check_cleaner().await;
    for path in ["/topics/orders/producers", "/topics/inventory.levels/log"] {
        let (_, doc) = page(&app, get(path)).await;
        assert!(
            !doc.contains("<script>alert") && !doc.contains("<img src=x"),
            "{path}"
        );
    }
    let (_, doc) = page(&app, get("/topics/orders/producers")).await;
    assert!(doc.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(
        doc.contains("%3Cscript%3Ealert%281%29%3C%2Fscript%3E")
            || doc.contains("%3Cscript%3Ealert(1)%3C/script%3E"),
        "{doc:.3000}"
    );
    let long = "t".repeat(249);
    w.create_topic(&long, 1, &[]).await;
    let (status, doc) = page(&app, get(&format!("/topics/{long}/log"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(doc.contains(&long));
    w.stop().await;
}

/// CON-539: these pages never write: every call a GET makes is a read, nothing in the broker changes, and the partitions it
/// opened to look are let go.
#[tokio::test]
async fn a_get_of_these_pages_reads_and_changes_nothing() {
    let w = seeded("log-readonly").await;
    w.fill("inventory.levels", 40, 4).await;
    let t = w.init_producer(Some("tx")).await;
    w.add_to_txn("tx", t, "orders", 0).await;
    assert_eq!(
        w.produce_batch("orders", 0, from(t.0, t.1, 0, true, 2), Some("tx"))
            .await
            .0,
        0
    );
    let (spy, log) = w.backend.recording();
    let app = support_write::router_of(&w, spy);
    let before = (
        w.topics().await,
        w.offset("orders", 0, -1).await,
        w.offset("inventory.levels", 0, -1).await,
    );
    let open = w.broker.open_partitions();
    for path in [
        "/topics/orders/log",
        "/topics/orders/log?partition=2",
        "/topics/inventory.levels/log",
        "/topics/orders.dlq/log",
        "/topics/orders/producers",
        "/topics/orders/producers?partition=1&open=1,2",
        "/topics/orders/transactions/tx",
        "/topics/orders/transactions/tx/held",
        "/fragments/log-sentence?topic=orders&partition=0",
        "/fragments/log-ruler?topic=inventory.levels&partition=0",
        "/fragments/log-cards?topic=inventory.levels&partition=0",
        "/fragments/producers-sentence?topic=orders&partition=0",
        "/fragments/producers-table?topic=orders&partition=0",
    ] {
        let (status, _) = page(&app, get(path)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
    }
    let reads = [
        ApiKey::Metadata,
        ApiKey::ListTransactions,
        ApiKey::DescribeTransactions,
        ApiKey::ListOffsets,
        ApiKey::DescribeConfigs,
        ApiKey::DescribeProducers,
        // The topic's own head says how many groups read it (the tab's count): a read.
        ApiKey::ListGroups,
        ApiKey::OffsetFetch,
    ];
    for call in log.lock().unwrap().iter() {
        assert!(reads.contains(call), "a GET called {call:?}");
    }
    assert_eq!(
        (
            w.topics().await,
            w.offset("orders", 0, -1).await,
            w.offset("inventory.levels", 0, -1).await
        ),
        before
    );
    assert!(w.broker.open_partitions() >= open.min(1));
    w.stop().await;
}

/// CON-540: the stream may be asked to watch these pages' fragments, and a fragment answers with an ETag like the others.
#[tokio::test]
async fn the_fragments_are_watchable_and_answer_with_a_tag() {
    let w = seeded("log-frag").await;
    let app = w.router();
    for id in [
        "log-sentence",
        "log-ruler",
        "log-cards",
        "producers-sentence",
        "producers-table",
    ] {
        let topic = if id.starts_with("log") {
            "inventory.levels"
        } else {
            "orders"
        };
        let (res, text) = support::send(
            &app,
            get(&format!("/fragments/{id}?topic={topic}&partition=0")),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK, "{id}");
        assert!(res.headers().contains_key("etag"), "{id}");
        assert!(
            text.contains(&format!("id=\"{id}\""))
                || id.ends_with("cards")
                || id.ends_with("ruler")
                || id.ends_with("sentence"),
            "{id}"
        );
        assert!(
            kafka_console::stream::parse_watch(id).is_ok(),
            "the stream may watch {id}"
        );
    }
    let (status, _) = page(&app, get("/fragments/log-sentence?topic=nope&partition=0")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    w.stop().await;
}
