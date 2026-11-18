//! The options of the Live filters.

use kafka_server::Answer;

use super::tests::{open, play, produce};
use super::*;
use crate::model::WireQuery;

#[test]
fn each_option_counts_the_rows_it_would_leave_among_the_other_filters() {
    let rec = Recorder::new();
    open(&rec, 1);
    open(&rec, 2);
    for (conn, client, topic) in [
        (1, "orders", "orders"),
        (1, "orders", "orders.dlq"),
        (2, "catalog", "orders"),
        (2, "catalog", "inventory"),
        (2, "catalog", "inventory"),
    ] {
        let (q, a) = produce(topic, 1, 0);
        play(&rec, conn, client, &q, Answer::Response(&a), 9);
    }
    let q = |s: &str| WireQuery::parse(&crate::form::parse(&format!("idle=0&{s}")));
    let counts = |v: &[crate::model::Facet]| -> Vec<(String, usize)> {
        v.iter().map(|f| (f.value.clone(), f.count)).collect()
    };
    let all = rec.tape(&q(""), now_ms()).facets;
    assert_eq!(
        counts(&all.topics),
        [
            ("inventory".into(), 1),
            ("orders".into(), 2),
            ("orders.dlq".into(), 1)
        ]
    );
    assert_eq!(
        counts(&all.clients),
        [("catalog".into(), 2), ("orders".into(), 2)]
    );
    assert_eq!(counts(&all.apis), [("Produce".into(), 4)]);
    // With a client chosen, the topics count what that client left; the clients count what the topics left (their own kind is not applied).
    let by_client = rec.tape(&q("client=catalog"), now_ms()).facets;
    assert_eq!(
        counts(&by_client.topics),
        [("inventory".into(), 1), ("orders".into(), 1)]
    );
    assert_eq!(
        counts(&by_client.clients),
        [("catalog".into(), 2), ("orders".into(), 2)]
    );
    let both = rec.tape(&q("client=catalog&topic=orders"), now_ms());
    assert_eq!(both.rows.len(), 1);
    assert_eq!(
        counts(&both.facets.clients),
        [("catalog".into(), 1), ("orders".into(), 1)]
    );
}

/// Live's Clients figure is the Clients sheet's (CON-1344): a client is a `client.id` with a connection open; its
/// connections are counted apart.
#[test]
fn live_counts_clients_as_the_clients_sheet_does() {
    let rec = Recorder::new();
    open(&rec, 1);
    open(&rec, 2);
    let (q, a) = produce("orders", 1, 0);
    play(&rec, 1, "orders", &q, Answer::Response(&a), 9);
    play(&rec, 2, "orders", &q, Answer::Response(&a), 9);
    let sheet = rec.clients(now_ms());
    let tape = rec.tape(&WireQuery::default(), now_ms());
    assert_eq!(
        (tape.clients, tape.connections),
        (sheet.rows.len(), sheet.connections)
    );
    assert_eq!((tape.clients, tape.connections), (1, 2));
}
