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
    (
        "/groups/search-indexer/members/search-indexer-1",
        "<h1 class=\"mono\">search-indexer</h1>",
        "/groups/search-indexer/members",
    ),
    (
        "/groups/analytics/reset",
        "<h1 class=\"mono\">analytics</h1>",
        "/groups/analytics/lag",
    ),
    (
        "/groups/search-indexer/delete",
        "<h1 class=\"mono\">search-indexer</h1>",
        "/groups/search-indexer",
    ),
    (
        "/groups/analytics/offsets/delete",
        "<h1 class=\"mono\">analytics</h1>",
        "/groups/analytics/lag?expiring=1",
    ),
    (
        "/topics/inventory.levels/config/edit?key=min.cleanable.dirty.ratio",
        "<h1 class=\"mono\">inventory.levels</h1>",
        "/topics/inventory.levels/config",
    ),
    (
        "/topics/inventory.levels/config/edit?key=cleanup.policy",
        "<h1 class=\"mono\">inventory.levels</h1>",
        "/topics/inventory.levels/config",
    ),
    (
        "/topics/inventory.levels/config/edit?key=min.compaction.lag.ms",
        "<h1 class=\"mono\">inventory.levels</h1>",
        "/topics/inventory.levels/config",
    ),
    (
        "/topics/inventory.levels/config/reset?key=segment.bytes",
        "<h1 class=\"mono\">inventory.levels</h1>",
        "/topics/inventory.levels/config",
    ),
    ("/live/99", "<h1>Live</h1>", "/live"),
    (
        "/live/99?client=orders-api",
        "<h1>Live</h1>",
        "/live?client=orders-api",
    ),
    ("/live/clients", "<h1>Live</h1>", "/live"),
    (
        "/settings/config/edit?key=log.retention.ms",
        "<h1>Settings</h1>",
        "/settings",
    ),
    ("/access/acls/new", "<h1>Access</h1>", "/access"),
    (
        "/access/acls/delete?type=topic&name=orders&pattern=literal&principal=User%3Aorders&host=%2A&op=write&perm=allow",
        "<h1>Access</h1>",
        "/access",
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
                local_links(&page).contains(&want),
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
    // The groups are in it, after the topics, each with the word that says which it is.
    assert!(
        alone.contains("href=\"/kafka/groups/search-indexer\" data-jump=\"search-indexer\""),
        "{alone}"
    );
    assert!(alone.contains("<span class=\"jump-kind\">group</span>"));
    assert!(alone.contains("<span class=\"jump-kind\">topic</span>"));
    assert!(
        alone.find("/kafka/topics/orders").unwrap()
            < alone.find("/kafka/groups/analytics").unwrap(),
        "topics first, then groups"
    );
    // With no script it is a page: the same names as plain links, and a filter that is a plain GET.
    let (res, page) = send(&app, get("/kafka/jump?q=inventory")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        page.contains("<h1>Jump to a topic or group</h1>"),
        "{page:.300}"
    );
    assert!(page.contains("/kafka/topics/inventory.levels"));
    assert!(
        !page.contains("href=\"/kafka/topics/orders\""),
        "the filter applies"
    );
    assert!(!page.contains("data-overlay"));
    // The filter applies to the groups too: no group is named like inventory, and one is named like search.
    assert!(!page.contains("/kafka/groups/"), "{page:.2000}");
    let (_, page) = send(&app, get("/kafka/jump?q=SEARCH")).await;
    assert!(page.contains("/kafka/groups/search-indexer"));
    assert!(!page.contains("/kafka/groups/analytics"));
}

/// The CSS the console ships, then what is outside the `prefers-reduced-motion: no-preference` blocks, then what is inside.
fn motion_rules() -> (String, String, String) {
    let css = include_str!("../assets/console.css");
    let mut inside = String::new();
    let mut outside = String::new();
    let mut rest = css;
    while let Some(at) = rest.find("@media (prefers-reduced-motion:no-preference){") {
        outside.push_str(&rest[..at]);
        let body = &rest[at..];
        // The block ends at the first "\n}" (the rules inside are one per line).
        let end = body.find("\n}\n").map_or(body.len(), |e| e + 2);
        inside.push_str(&body[..end]);
        rest = &body[end..];
    }
    outside.push_str(rest);
    (css.to_owned(), outside, inside)
}

#[test]
fn the_motion_is_gentle_and_only_for_people_who_have_not_asked_for_less() {
    let (css, outside, motion) = motion_rules();
    // The numbers of the UX brief: scrim 180 ms, drawer 240 ms (translateX(100%) to 0), dialog 180 ms (scale .98), closing 180 ms; the page 180 ms.
    for want in [
        ".ovl-in .scrim{animation:ovl-fade 180ms ease-out both}",
        ".ovl-in.ovl-drawer .drawer-panel{animation:ovl-slide 240ms cubic-bezier(.22,1,.36,1) both}",
        ".ovl-in.ovl-dialog .modal{animation:ovl-pop 180ms cubic-bezier(.22,1,.36,1) both}",
        ".ovl-out .scrim{animation:ovl-fade-out 180ms ease-in both}",
        ".ovl-out.ovl-drawer .drawer-panel{animation:ovl-slide-out 180ms cubic-bezier(.4,0,1,1) both}",
        ".ovl-out.ovl-dialog .modal{animation:ovl-pop-out 160ms ease-in both}",
        "@view-transition{navigation:auto}",
        ".topbar{view-transition-name:topbar}",
        ".rail{view-transition-name:rail}",
        "::view-transition-new(main){animation:vt-in 180ms cubic-bezier(.22,1,.36,1) both}",
    ] {
        assert!(
            motion.contains(want),
            "{want:?} must be inside the no-preference block"
        );
    }
    // Nothing that moves is outside it, and only opacity and transform are in the keyframes (no layout).
    assert!(!outside.contains("animation:ovl-") && !outside.contains("view-transition-name"));
    for frames in css.split("@keyframes ").skip(1) {
        let body = &frames[..frames.find("}}").unwrap_or(frames.len())];
        for banned in ["width", "height", "top", "left", "margin", "padding"] {
            assert!(!body.contains(&format!("{banned}:")), "{body}: {banned}");
        }
    }
    // And a person who asks for less gets none, whatever else is added later.
    assert!(css.contains("@media (prefers-reduced-motion:reduce){*{animation:none!important;transition:none!important}}"));
}
