//! Overlays (`src/ui/overlay.rs`, `docs/port/console/overlays.md`): every drawer and dialog has an address of its own, and a
//! direct visit to it is the PARENT page with the overlay open on it (so a reload, a shared link and a browser with no script
//! all work); a script is sent the overlay alone; closing is a plain link to the parent; hostile names are inert.
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

/// An overlay's address, the parent's heading it must sit over, and the parent's address that closing returns to.
const OVERLAYS: &[(&str, &str, &str)] = &[
    (
        "/topics/orders/messages/1/16050",
        "<h1 class=\"mono\">orders</h1>",
        "/topics/orders",
    ),
    (
        "/topics/orders/messages/2/16056",
        "<h1 class=\"mono\">orders</h1>",
        "/topics/orders",
    ),
    (
        "/topics/orders/produce",
        "<h1 class=\"mono\">orders</h1>",
        "/topics/orders",
    ),
    (
        "/topics/orders/delete",
        "<h1 class=\"mono\">orders</h1>",
        "/topics/orders",
    ),
    (
        "/topics/orders/partitions/add",
        "<h1 class=\"mono\">orders</h1>",
        "/topics/orders/partitions",
    ),
    (
        "/topics/orders/delete-records?partition=0",
        "<h1 class=\"mono\">orders</h1>",
        "/topics/orders/partitions",
    ),
];

fn between<'a>(text: &'a str, from: &str, to: &str) -> &'a str {
    let a = text
        .find(from)
        .unwrap_or_else(|| panic!("{from:?} not found"))
        + from.len();
    let b = text[a..].find(to).unwrap() + a;
    &text[a..b]
}

#[tokio::test]
async fn a_direct_visit_is_the_parent_page_with_the_overlay_open_on_it() {
    for prefix in ["/", "/kafka"] {
        let (app, state) = app(prefix, &[]);
        for (path, heading, close) in OVERLAYS {
            let url = state.base.url(path);
            let (res, page) = send(&app, get(&url)).await;
            assert_eq!(res.status(), StatusCode::OK, "{url}");
            // The parent's landmarks, its heading and its own address in the breadcrumb.
            assert!(
                page.starts_with("<!DOCTYPE html>") && page.contains("<nav class=\"rail\""),
                "{url}: the shell"
            );
            assert!(
                page.contains("<main class=\"main\" id=\"main\">"),
                "{url}: the parent's main"
            );
            assert!(
                page.contains(heading) || page.contains(&heading.replace(" class=\"mono\"", "")),
                "{url}: the parent's heading {heading}"
            );
            assert_eq!(
                page.matches("<h1").count(),
                1,
                "{url}: one h1, the parent's"
            );
            // The page behind is inert and the overlay is the last thing in the body, open and named.
            assert!(page.contains("<div class=\"root\" inert>"), "{url}: inert");
            let overlay = &page[page.find("data-overlay").unwrap()..];
            assert!(
                overlay.contains("role=\"dialog\" aria-modal=\"true\" aria-labelledby=\""),
                "{url}"
            );
            let label = between(overlay, "aria-labelledby=\"", "\"");
            assert!(
                overlay.contains(&format!("id=\"{label}\"")) && overlay.contains("<h2 "),
                "{url}: the panel is named by a heading that is there ({label})"
            );
            assert!(
                page.find("<main").unwrap() < page.find("data-overlay").unwrap(),
                "{url}: the overlay follows the parent"
            );
            // Closing is a link to the parent, with no script: the scrim and the control say the same address.
            let want = state.base.url(close);
            assert_eq!(
                between(&page, "<a class=\"scrim\" href=\"", "\""),
                want,
                "{url}"
            );
            assert!(
                local_links(&page).iter().any(|l| *l == want),
                "{url}: {want} is linked"
            );
            assert!(
                page.contains(&format!("data-close=\"{want}\"")),
                "{url}: the script reads the same address"
            );
        }
    }
}

#[tokio::test]
async fn a_script_is_sent_the_overlay_alone_marked_as_one() {
    let (app, _) = app("/", &[]);
    for (path, _, close) in OVERLAYS {
        let mut ask = get(path);
        ask.headers_mut().insert("x-fragment", "1".parse().unwrap());
        let (res, alone) = send(&app, ask).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert_eq!(res.headers()["x-overlay"], "1", "{path}");
        assert!(
            alone.starts_with("<div class=\"ovl ovl-")
                && alone.contains("data-overlay")
                && !alone.contains("<html")
                && !alone.contains("<nav class=\"rail\"")
                && alone.contains(&format!("data-close=\"{close}")),
            "{path}: {alone:.200}"
        );
    }
}

#[tokio::test]
async fn the_records_parent_keeps_its_query_and_the_drawer_closes_back_to_it() {
    let (app, _) = app("/", &[]);
    let (_, page) = send(
        &app,
        get("/topics/orders/messages/1/16050?partition=1&isolation=read_committed"),
    )
    .await;
    let scrim = between(&page, "<a class=\"scrim\" href=\"", "\"").replace("&amp;", "&");
    assert_eq!(scrim, "/topics/orders?partition=1&isolation=read_committed");
    // The parent behind it is that filtered page.
    assert!(page.contains("read_committed"), "{page:.100}");
    // A row of the table opens the drawer over the same page: the link carries the page's query.
    let (_, parent) = send(&app, get("/topics/orders?partition=0&offset=16098")).await;
    assert!(
        local_links(&parent)
            .iter()
            .any(|l| l.starts_with("/topics/orders/messages/0/")
                && l.ends_with("?partition=0&offset=16098")),
        "a row opens the record with the query of its page"
    );
    assert!(
        parent.contains("data-dialog>16,103</a>"),
        "the offset is an opener"
    );
    // And the buttons that open a dialog are openers too, on the pages that carry them.
    let (_, topic) = send(&app, get("/topics/orders")).await;
    assert_eq!(
        topic
            .matches("href=\"/topics/orders/produce\" data-dialog")
            .count(),
        1
    );
    assert_eq!(
        topic
            .matches("href=\"/topics/orders/delete\" data-dialog")
            .count(),
        1
    );
    let (_, parts) = send(&app, get("/topics/orders/partitions")).await;
    assert!(parts.contains("/delete-records?partition=0\" data-dialog"));
}

#[tokio::test]
async fn the_overlays_of_a_hostile_topic_name_are_text_in_the_page_that_says_there_is_none() {
    let (app, _) = app("/", &[]);
    for path in [
        "/topics/%3Cimg%20src=x%20onerror=alert(1)%3E/messages/1/2",
        "/topics/%3Cimg%20src=x%20onerror=alert(1)%3E/produce",
        "/topics/%3Cimg%20src=x%20onerror=alert(1)%3E/delete",
        "/topics/%3Cimg%20src=x%20onerror=alert(1)%3E/partitions/add",
        "/topics/%3Cimg%20src=x%20onerror=alert(1)%3E/delete-records",
    ] {
        let (res, page) = send(&app, get(path)).await;
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
async fn an_overlay_with_no_parent_topic_is_the_page_that_says_so() {
    let (app, _) = app("/", &[]);
    let (res, page) = send(&app, get("/topics/no.such.topic/produce")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(page.contains("No such topic") || page.contains("There is no topic called"));
}

#[tokio::test]
async fn the_jump_palette_is_an_overlay_alone_to_a_script_and_a_page_of_links_to_a_browser() {
    let (app, _) = app("/kafka", &[]);
    let mut ask = get("/kafka/jump");
    ask.headers_mut().insert("x-fragment", "1".parse().unwrap());
    let (res, alone) = send(&app, ask).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["x-overlay"], "1");
    assert!(
        alone.starts_with("<div class=\"ovl ovl-dialog root\" data-overlay data-transient"),
        "{alone:.200}"
    );
    assert!(
        alone.contains("aria-labelledby=\"jump-title\"") && alone.contains("id=\"jump-title\"")
    );
    assert!(
        alone.contains("href=\"/kafka/topics/orders\" data-jump=\"orders\""),
        "{alone}"
    );
    assert!(!alone.contains("<html"));
    // With no script it is a page: the same names as plain links, and a filter that is a plain GET.
    let (res, page) = send(&app, get("/kafka/jump?q=inventory")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(page.contains("<h1>Jump to a topic</h1>"), "{page:.300}");
    assert!(page.contains("/kafka/topics/inventory.levels"));
    assert!(
        !page.contains("href=\"/kafka/topics/orders\""),
        "the filter applies"
    );
    assert!(!page.contains("data-overlay"));
}
