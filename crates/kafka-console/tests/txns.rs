//! Home must see every open transaction (audit A1): a real broker with many transactions open, the ones near their
//! timeout last in the order the broker lists them, and the page must still say they need someone. Where it has to stop
//! looking it says so, and never "Nothing needs you" for transactions it did not look at.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod world;

use std::time::Duration;

use kafka_console::api::HomeResponse;
use kafka_console::gather;
use kafka_console::model::HomeState;
use kafka_console::source::now_ms;
use support::{api_get, send};
use world::{World, world};

/// `count` transactions that will not time out for ten minutes, with ids that sort first.
async fn long_ones(w: &World, count: usize) {
    for i in 0..count {
        w.open_txn(&format!("a-{i:03}"), 600_000).await;
    }
}

/// Three transactions about to time out, with ids that sort after every `a-...`.
async fn short_ones(w: &World) {
    for i in 0..3 {
        w.open_txn(&format!("z-{i}"), 900).await;
    }
    tokio::time::sleep(Duration::from_millis(700)).await;
}

#[tokio::test]
async fn forty_four_open_transactions_do_not_hide_three_that_are_near_their_timeout() {
    let w = world("many", |_, _| {}).await;
    w.create_topic("orders").await;
    long_ones(&w, 44).await;
    short_ones(&w).await;
    let (_, body) = send(&w.router(), api_get("/api/v1/home")).await;
    let home: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        home["home"]["state"]["kind"], "attention",
        "three transactions need someone and Home says nothing does: {body:.600}"
    );
    assert_eq!(
        home["words"]["attention_headline"],
        "3 things need you: 3 transactions are close to their timeout.",
        "{body:.600}"
    );
    w.stop().await;
}

#[tokio::test]
async fn twenty_five_open_transactions_are_all_looked_at() {
    let w = world("twenty-five", |_, _| {}).await;
    w.create_topic("orders").await;
    long_ones(&w, 24).await;
    w.open_txn("z-last", 900).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let seen = gather::nearest(&w.backend, now_ms()).await;
    let a = seen.attention.expect("the 25th is near its timeout");
    assert_eq!((a.count, a.transactional_id.as_str()), (1, "z-last"));
    assert_eq!(seen.unchecked, 0);
    w.stop().await;
}

#[tokio::test]
async fn where_it_has_to_stop_looking_it_says_so_and_never_claims_the_all_clear() {
    let w = world("capped", |_, _| {}).await;
    w.create_topic("orders").await;
    long_ones(&w, 12).await;
    short_ones(&w).await;
    // A cap of ten: the first ten ids are described, five open transactions are not.
    let seen = gather::nearest_capped(&w.backend, now_ms(), 10).await;
    assert_eq!(seen.unchecked, 5);
    assert!(
        seen.attention.is_none(),
        "the near ones sort last and were not looked at"
    );
    // What Home says, built from that, says what it did not look at.
    let mut v = kafka_console::fixtures::home(HomeState::Steady);
    v.unchecked = seen.unchecked;
    let words = HomeResponse::new(v.clone()).words;
    assert_eq!(
        words.attention_headline, None,
        "no all-clear is claimed either"
    );
    assert_eq!(
        words.unchecked_note.as_deref(),
        Some("5 open transactions were not checked.")
    );
    // And a cap that is met by what it found: "or more".
    let mut a = kafka_console::fixtures::attention();
    a.count = 2;
    v.state = HomeState::Attention(a);
    let words = HomeResponse::new(v).words;
    assert_eq!(
        words.attention_headline.as_deref(),
        Some("At least 2 transactions are close to their timeout.")
    );
    assert_eq!(
        words.unchecked_note.as_deref(),
        Some("5 open transactions were not checked.")
    );
    w.stop().await;
}
