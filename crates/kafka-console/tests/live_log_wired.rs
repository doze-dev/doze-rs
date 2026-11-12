//! The Log and Producers pages with what the wire recorder adds (slice 9e), over a real broker in this process: the topic's "On
//! the wire" dock under both pages, and the name of a producer ("Seen as") from the requests the recorder saw it write.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_live;
mod support_log;
mod support_write;

use kafka_wire::messages::ApiKey;
use support::get;
use support_live::{Wire, produce, recorded, router, versions};
use support_log::from;
use support_write::page;

/// CON-587: a producer is named by the client the recorder saw write with its id, and the dock lists what touched the topic.
#[tokio::test]
async fn a_producer_is_named_by_the_client_that_wrote_and_the_dock_is_under_both_pages() {
    let (w, recorder) = recorded("log-wired", false).await;
    w.create_topic("orders", 1, &[]).await;
    let (pid, epoch) = w.init_producer(None).await;
    let mut app_wire = Wire::connect(&w).await;
    app_wire
        .call(ApiKey::ApiVersions, 3, "payments-app", &versions("sarama"))
        .await;
    app_wire
        .call(
            ApiKey::Produce,
            9,
            "payments-app",
            &produce("orders", from(pid, epoch, 0, false, 3), None),
        )
        .await;
    // A second producer that the recorder never saw write: its id has no name.
    let (quiet, quiet_epoch) = w.init_producer(None).await;
    assert_eq!(
        w.produce_batch("orders", 0, from(quiet, quiet_epoch, 0, false, 2), None)
            .await
            .0,
        0
    );
    let app = router(&w, &recorder);
    let (_, producers) = page(&app, get("/topics/orders/producers")).await;
    assert!(
        producers.contains("payments-app (go)") || producers.contains("payments-app ("),
        "{producers:.3000}"
    );
    assert!(
        producers.contains("not known"),
        "the producer the recorder never saw write has no name"
    );
    let (_, log) = page(&app, get("/topics/orders/log")).await;
    for (name, doc) in [("producers", &producers), ("log", &log)] {
        assert!(
            doc.contains("<h2>On the wire</h2>"),
            "{name}: the dock is under the page"
        );
        assert!(
            doc.contains("payments-app"),
            "{name}: the request that touched the topic is in the dock"
        );
        assert!(
            doc.contains("data-fragment=\"topic-wire\"") || doc.contains("topic-wire"),
            "{name}: the dock is watched"
        );
    }
    w.stop().await;
}
