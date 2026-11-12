//! More overlays (`tests/overlays.rs` has the table): the Groups and Config dialogs, their openers and refusals, hostile group and
//! member names, and the Live drawers over the Live page with its filters.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use axum::http::StatusCode;
use support::{app, get, local_links, send};

fn between<'a>(text: &'a str, from: &str, to: &str) -> &'a str {
    let a = text
        .find(from)
        .unwrap_or_else(|| panic!("{from:?} not found"))
        + from.len();
    let b = text[a..].find(to).unwrap() + a;
    &text[a..b]
}

#[tokio::test]
async fn the_overlays_of_a_hostile_group_or_member_name_are_text_in_the_page_that_says_there_is_none()
 {
    let (app, _) = app("/", &[]);
    let bad = "%3Cimg%20src=x%20onerror=alert(1)%3E";
    for path in [
        format!("/groups/{bad}/reset"),
        format!("/groups/{bad}/delete"),
        format!("/groups/{bad}/offsets/delete"),
        format!("/groups/{bad}/members/{bad}"),
        format!("/groups/search-indexer/members/{bad}"),
        format!("/topics/{bad}/config/edit?key=segment.ms"),
        format!("/topics/{bad}/config/reset?key=segment.ms"),
    ] {
        let (res, page) = send(&app, get(&path)).await;
        assert!(
            res.status() == StatusCode::NOT_FOUND || res.status() == StatusCode::OK,
            "{path} {}",
            res.status()
        );
        assert!(!page.contains("<img"), "{path}: {page:.300}");
        assert!(
            !page.contains("data-overlay"),
            "{path}: no overlay over a page that is not there"
        );
    }
}

#[tokio::test]
async fn the_buttons_that_open_a_group_dialog_or_a_member_are_openers() {
    let (app, _) = app("/kafka", &[]);
    let (_, lag) = send(&app, get("/kafka/groups/analytics/lag")).await;
    for want in [
        "href=\"/kafka/groups/analytics/reset\" data-dialog",
        "href=\"/kafka/groups/analytics/delete\" data-dialog",
    ] {
        assert!(lag.contains(want), "{want}");
    }
    let (_, expiring) = send(&app, get("/kafka/groups/analytics/lag?expiring=1")).await;
    assert!(expiring.contains("href=\"/kafka/groups/analytics/offsets/delete\" data-dialog"));
    let (_, members) = send(&app, get("/kafka/groups/search-indexer/members")).await;
    assert!(
        members
            .contains("href=\"/kafka/groups/search-indexer/members/search-indexer-1\" data-dialog"),
        "a member row opens the drawer"
    );
    let (_, group) = send(&app, get("/kafka/groups/search-indexer")).await;
    assert!(group.contains("href=\"/kafka/groups/search-indexer/delete\" data-dialog"));
    // The Config page's Edit and Reset were openers already; the dialogs close back to it.
    let (_, config) = send(&app, get("/kafka/topics/inventory.levels/config")).await;
    assert!(config.contains("/config/edit?key=segment.ms\" data-dialog"));
}

#[tokio::test]
async fn a_refusal_in_a_dialog_answers_the_dialog_over_its_parent_and_a_result_is_a_page() {
    let (app, _) = app("/", &[]);
    // A group with a member refuses to forget a topic it reads: a script is sent the dialog again, marked as an overlay,
    // with Kafka's own words in it.
    let body = "topic=clickstream";
    let mut ask = support::post("/groups/search-indexer/offsets/delete", body);
    ask.headers_mut().insert("x-fragment", "1".parse().unwrap());
    let (res, alone) = send(&app, ask).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["x-overlay"], "1");
    assert!(alone.contains("id=\"delete-offsets-title\"") && !alone.contains("<html"));
    assert!(alone.contains("GROUP_SUBSCRIBED_TO_TOPIC"), "{alone}");
    // A browser with no script is sent the same dialog over the Lag page.
    let (res, page) = send(
        &app,
        support::post("/groups/search-indexer/offsets/delete", body),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(page.contains("<div class=\"root\" inert>") && page.contains("data-overlay"));
    assert!(page.contains("<title>"), "a whole document");
    // A write that worked is a redirect to the page that shows it, not an overlay.
    let (res, _) = send(
        &app,
        support::post("/groups/analytics/offsets/delete", "topic=orders"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn a_live_drawer_closes_to_the_live_page_with_its_filters_and_the_tape_links_open_it_in_place()
 {
    for prefix in ["/", "/kafka"] {
        let (app, state) = app(prefix, &[]);
        // The tape's time links are openers that carry the page's own filters.
        let (_, live) = send(&app, get(&state.base.url("/live?refused=1"))).await;
        let link = local_links(&live)
            .into_iter()
            .find(|l| l.contains("/live/") && !l.contains("/live/clients"))
            .expect("a request link on the tape");
        assert!(
            link.ends_with("refused=1") || link.contains("refused=1"),
            "{link}"
        );
        assert!(
            live.contains(&format!("href=\"{link}\" data-dialog")),
            "{link}"
        );
        // Following it is the Live page with the drawer open, and Close is that page's address with the same filters.
        let (res, page) = send(&app, get(&link)).await;
        assert_eq!(res.status(), StatusCode::OK, "{link}");
        assert!(
            page.contains("<h1>Live</h1>") && page.contains("<div class=\"root\" inert>"),
            "{link}"
        );
        let want = state.base.url("/live?refused=1");
        assert_eq!(between(&page, "<a class=\"scrim\" href=\"", "\""), want);
        // The Live page behind it is the tape fragment (with its row count), so a script can hold it and let it catch up.
        assert!(
            page.contains("id=\"live-wire\"") && page.contains("data-seen="),
            "{link}"
        );
        // A script is sent the drawer alone.
        let mut ask = get(&link);
        ask.headers_mut().insert("x-fragment", "1".parse().unwrap());
        let (res, alone) = send(&app, ask).await;
        assert_eq!(res.headers()["x-overlay"], "1");
        assert!(
            alone.starts_with("<div class=\"ovl ovl-drawer root\"") && !alone.contains("<html")
        );
    }
}

#[tokio::test]
async fn a_request_that_is_not_on_the_tape_and_a_hostile_id_are_the_page_that_says_so_with_no_drawer()
 {
    let (app, _) = app("/", &[]);
    for path in [
        "/live/123456789",
        "/live/%3Cimg%20src=x%20onerror=alert(1)%3E",
    ] {
        let (res, page) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}");
        assert!(
            !page.contains("<img") && !page.contains("data-overlay"),
            "{path}"
        );
    }
}
