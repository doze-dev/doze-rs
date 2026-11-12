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
