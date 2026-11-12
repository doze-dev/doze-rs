//! The Live page's filters over the fixtures (CON-760): each one changes the rows, as it does over a real tape
//! (`live_wire::every_filter_of_the_live_page_changes_the_rows_of_a_real_tape`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use axum::http::StatusCode;
use support::{app, get, send};

async fn page(path: &str) -> (StatusCode, String) {
    let (router, _) = app("", &[]);
    let (res, body) = send(&router, get(path)).await;
    (res.status(), body)
}

/// CON-760: every filter of the Live page changes the rows (the fixtures tape once ignored the query, so no filter did anything).
#[tokio::test]
async fn every_filter_of_the_live_page_changes_the_rows() {
    let rows = |doc: &str| doc.matches("<tr").count();
    let (_, all) = page("/live").await;
    for (q, fewer) in [
        ("?client=orders", true),
        ("?topic=orders", true),
        ("?api=Fetch", true),
        ("?refused=1", true),
        ("?q=fulfilment", true),
        ("?idle=0", false),
    ] {
        let (status, doc) = page(&format!("/live{q}")).await;
        assert_eq!(status, StatusCode::OK, "{q}");
        if fewer {
            assert!(
                rows(&doc) < rows(&all),
                "{q}: the filter changed nothing ({} rows)",
                rows(&doc)
            );
            assert!(rows(&doc) > 1, "{q}: the filter left no row");
        } else {
            assert!(
                doc.contains("no records"),
                "{q}: the polls that found nothing are shown"
            );
        }
    }
    assert!(
        page("/live?client=orders")
            .await
            .1
            .contains("data-frag=\"/fragments/live-wire?client=orders\""),
        "the stream's fragment keeps the filter"
    );
}

/// CON-857: the Groups state filter and the Messages partitions take several values: the address repeats the field, the control
/// shows each as chosen, and the rows are those of any chosen state.
#[tokio::test]
async fn groups_states_and_messages_partitions_take_several_values() {
    let rows = |doc: &str| doc.matches("<tr id=\"g-").count();
    let (_, all) = page("/groups").await;
    let (_, stable) = page("/groups?state=stable").await;
    let (_, empty) = page("/groups?state=empty").await;
    let (_, both) = page("/groups?state=stable&state=empty").await;
    assert!(
        rows(&stable) > 0 && rows(&empty) > 0,
        "{} {}",
        rows(&stable),
        rows(&empty)
    );
    assert_eq!(
        rows(&both),
        rows(&stable) + rows(&empty),
        "any of the states"
    );
    assert!(rows(&both) < rows(&all) || rows(&all) == rows(&both));
    for v in ["stable", "empty"] {
        assert!(
            both.contains(&format!(r#"name="state" value="{v}" checked"#)),
            "{v} is shown as chosen"
        );
    }
    assert!(
        both.contains(r#"<span class="dz-value">Stable, Empty</span>"#),
        "the button says them"
    );
    let (_, msgs) = page("/topics/orders/messages?partition=0&partition=1").await;
    assert!(
        msgs.contains("Partition 0") && msgs.contains("Partition 1"),
        "a chip for each"
    );
    assert!(
        msgs.contains(r#"name="partition" value="0" checked"#)
            && msgs.contains(r#"name="partition" value="1" checked"#),
        "the multi-select has both"
    );
}

/// CON-854: two chosen values are tags that take themselves off, and an empty tape says which filters are on.
#[tokio::test]
async fn the_empty_tape_names_the_filters_and_two_values_make_tags() {
    let (_, doc) = page("/live?topic=orders&topic=clickstream").await;
    assert!(
        doc.contains(r#"id="live-picked""#) && doc.matches(r#"class="dz-tag""#).count() == 2,
        "{doc:.3000}"
    );
    assert!(doc.contains("Clear filters"));
    let (_, one) = page("/live?topic=orders").await;
    assert!(
        !one.contains(r#"class="dz-tag""#),
        "one value is on the button, not in tags"
    );
    let (_, none) = page("/live?client=no-such-app&topic=orders&topic=clickstream").await;
    assert!(
        none.contains(r#"id="live-no-match""#) && none.contains("No request matches these filters"),
        "{none:.3000}"
    );
    assert!(
        none.contains("app is no-such-app") && none.contains("topic is orders or clickstream"),
        "{none:.3000}"
    );
}
