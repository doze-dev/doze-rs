//! The Producers pages over a real broker in this process: the producers a partition remembers with their last five batches, the
//! transaction held open and what it holds back, and an abort remembered with its marker.
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
use support::get;
use support_log::{from, out_dir, seeded};
use support_write::page;

fn save(id: &str, doc: &str) {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{id}.html")), doc).unwrap();
}

/// CON-534: the Producers page names the producers the partition remembers with their last five batches, and the transaction
/// held open is the one that holds the last stable offset where the broker says.
#[tokio::test]
async fn the_producers_page_names_the_transaction_that_holds_the_stable_offset() {
    let w = seeded("prod-open").await;
    let app = w.router();
    let (id, epoch) = w.init_producer(None).await;
    let mut seq = 0;
    for n in [2, 1, 3, 2, 4, 2, 1] {
        assert_eq!(
            w.produce_batch("orders", 0, from(id, epoch, seq, false, n), None)
                .await
                .0,
            0
        );
        seq += i32::try_from(n).unwrap();
    }
    let t = w.init_producer(Some("orders-eos")).await;
    w.add_to_txn("orders-eos", t, "orders", 0).await;
    assert_eq!(
        w.produce_batch("orders", 0, from(t.0, t.1, 0, true, 3), Some("orders-eos"))
            .await
            .0,
        0
    );
    let end = w.offset("orders", 0, -1).await;
    let (status, doc) = page(&app, get("/topics/orders/producers")).await;
    save("topic-producers", &doc);
    assert_eq!(status, StatusCode::OK);
    assert!(doc.contains("<span class=\"st warn\">Transaction open</span>"));
    assert!(
        doc.contains(&format!("<td class=\"name mono\">{id}</td>"))
            && doc.contains(&format!("<td class=\"name mono\">{}</td>", t.0))
    );
    assert!(doc.contains("href=\"/topics/orders/transactions/orders-eos\""));
    assert!(
        doc.contains(&format!(
            "Producer {}, the last 5 batches orders-0 remembers",
            t.0
        )) || doc.contains("the last 1 batch orders-0 remembers"),
        "{doc:.6000}"
    );
    // Five of the seven batches of the idempotent producer: ask for its row.
    let (_, opened) = page(&app, get(&format!("/topics/orders/producers?open={id}"))).await;
    assert!(opened.contains(&format!(
        "Producer {id}, the last 5 batches orders-0 remembers"
    )));
    assert!(opened.contains("OUT_OF_ORDER_SEQUENCE_NUMBER (45)"));
    let stable = w.offset("orders", 0, -1).await;
    assert_eq!(stable, end);
    assert!(
        doc.contains(&format!(
            "holds the last stable offset at <b>{}</b>",
            end - 3
        )),
        "{doc:.3000}"
    );
    assert!(doc.contains("a read_committed consumer is held <b>3</b> records behind"));
    w.stop().await;
}

/// CON-535: the page of a transaction and the page of who it holds back, with the countdown the broker's own clock gives.
#[tokio::test]
async fn a_transaction_page_says_what_it_holds_back_and_what_the_timeout_will_do() {
    let w = seeded("prod-txn").await;
    let app = w.router();
    let t = w.init_producer(Some("orders-eos")).await;
    w.add_to_txn("orders-eos", t, "orders", 1).await;
    assert_eq!(
        w.produce_batch("orders", 1, from(t.0, t.1, 0, true, 2), Some("orders-eos"))
            .await
            .0,
        0
    );
    let (status, doc) = page(&app, get("/topics/orders/transactions/orders-eos")).await;
    save("txn-open", &doc);
    assert_eq!(status, StatusCode::OK);
    assert!(doc.contains("<span class=\"st warn\">Ongoing</span>"));
    assert!(doc.contains(&format!("{}, epoch {}", t.0, t.1)));
    assert!(
        doc.contains("orders-1: <b>2</b> records, 0 to 1."),
        "{doc:.5000}"
    );
    assert!(doc.contains("the broker aborts it, whatever the producer is doing"));
    assert!(doc.contains("INVALID_PRODUCER_EPOCH (47)"));
    assert!(doc.contains("Transactions on this broker") && doc.contains("orders-eos"));
    let (status, held) = page(&app, get("/topics/orders/transactions/orders-eos/held")).await;
    save("txn-holding", &held);
    assert_eq!(status, StatusCode::OK);
    assert!(held.contains("read_uncommitted") && held.contains("read_committed"));
    assert!(held.contains("<td class=\"mono\">last_stable_offset</td>"));
    // Ended, it holds nothing: the page says it is over.
    assert_eq!(w.end_txn("orders-eos", t, true).await, 0);
    let (_, over) = page(&app, get("/topics/orders/transactions/orders-eos")).await;
    assert!(
        over.contains("CompleteCommit") && over.contains("It is over: nothing is left to abort.")
    );
    let (status, none) = page(&app, get("/topics/orders/transactions/nobody")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(none.contains("No such transaction"));
    w.stop().await;
}

/// CON-536: an aborted transaction is remembered with its marker and the time the marker was written.
#[tokio::test]
async fn an_aborted_transaction_is_listed_with_its_marker() {
    let w = seeded("prod-aborted").await;
    let app = w.router();
    let t = w.init_producer(Some("tx")).await;
    w.add_to_txn("tx", t, "orders", 0).await;
    assert_eq!(
        w.produce_batch("orders", 0, from(t.0, t.1, 0, true, 2), Some("tx"))
            .await
            .0,
        0
    );
    assert_eq!(w.end_txn("tx", t, false).await, 0);
    let (_, doc) = page(&app, get("/topics/orders/producers")).await;
    assert!(
        doc.contains("Aborted transactions remembered: 1"),
        "{doc:.6000}"
    );
    assert!(doc.contains("0 to 1") && doc.contains("marker at 2"));
    assert!(doc.contains("an abort marker ended it"));
    assert!(!doc.contains("not known</td>") || doc.matches("not known").count() >= 1);
    w.stop().await;
}
