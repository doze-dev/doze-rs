//! The pages as a person meets them (UX pass, CON-700 and on), over the fixtures: the breadcrumb is the entity path in every
//! state, a screen this edition lacks is one designed page, the five tabs of a topic share one header, the Messages toolbar is
//! one layout in every state, and no page has an expander that is a link.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use axum::http::{StatusCode, header};
use support::{app, get, send};

async fn page(path: &str) -> (StatusCode, String) {
    let (router, _) = app("", &[]);
    let (res, body) = send(&router, get(path)).await;
    (res.status(), body)
}

fn between<'a>(doc: &'a str, from: &str, to: &str) -> &'a str {
    let a = doc.find(from).unwrap_or_else(|| panic!("no {from}"));
    let rest = &doc[a..];
    &rest[..rest.find(to).unwrap_or(rest.len())]
}

/// The steps of the breadcrumb, as `(text, href)`.
fn crumbs(doc: &str) -> Vec<(String, Option<String>)> {
    let nav = between(doc, "<nav class=\"crumbs\"", "</nav>");
    let mut out = Vec::new();
    let mut rest = nav;
    loop {
        let a = rest.find("<a href=\"");
        let s = rest.find("<span class=\"cur\">");
        match (a, s) {
            (Some(a), Some(s)) if a < s => {
                let (step, next) = link_step(rest, a);
                out.push(step);
                rest = next;
            }
            (Some(a), None) => {
                let (step, next) = link_step(rest, a);
                out.push(step);
                rest = next;
            }
            (_, Some(s)) => {
                let t = &rest[s + 18..];
                out.push((unescape(&t[..t.find("</span>").unwrap()]), None));
                rest = &t[t.find("</span>").unwrap()..];
            }
            (None, None) => break,
        }
    }
    out
}

fn link_step(rest: &str, at: usize) -> ((String, Option<String>), &str) {
    let t = &rest[at + 9..];
    let href = &t[..t.find('"').unwrap()];
    let text = &t[t.find('>').unwrap() + 1..];
    let end = text.find("</a>").unwrap();
    ((unescape(&text[..end]), Some(unescape(href))), &text[end..])
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn title(doc: &str) -> String {
    unescape(
        between(doc, "<title>", "</title>")
            .trim_start_matches("<title>")
            .trim_end_matches(" · doze-kafka"),
    )
}

/// The paths of the fixture world that exercise every state a breadcrumb used to carry.
const PAGES: &[&str] = &[
    "/",
    "/?state=quiet",
    "/?state=attention",
    "/connect",
    "/connect/advertised",
    "/connect/first-request",
    "/topics",
    "/topics?q=refunds",
    "/topics?q=zzz",
    "/topics/new",
    "/topics/new?step=config",
    "/topics/new?step=review",
    "/topics/orders",
    "/topics/orders/messages?partition=0",
    "/topics/orders/messages?partition=0&offset=16098",
    "/topics/orders/messages?isolation=read_committed",
    "/topics/orders/messages?partition=1&isolation=read_committed",
    "/topics/orders/messages?key=ord-88412",
    "/topics/orders/messages/0/16103",
    "/topics/orders.dlq",
    "/topics/notifications.email",
    "/topics/orders/partitions",
    "/topics/orders/config",
    "/topics/orders/config?show=changed",
    "/topics/orders/consumers",
    "/topics/orders/wire",
    "/topics/orders/log",
    "/topics/orders/producers",
    "/topics/nope",
    "/groups",
    "/groups/fulfilment",
    "/groups/fulfilment/members",
    "/groups/fulfilment/lag",
    "/groups/analytics/lag?expiring=1",
    "/groups/search-indexer/lag",
    "/groups/fulfilment/wire",
    "/groups/fulfilment/story",
    "/groups/nope",
    "/live",
    "/live?state=paused",
    "/live?client=orders",
    "/live/clients",
    "/live/12",
    "/access",
    "/lab",
    "/settings",
    "/jump",
    "/nowhere",
];

const MODES: &[&str] = &[
    "read_committed",
    "read_uncommitted",
    "paused",
    "a busy minute",
    "nothing yet",
    "filtered",
    "all partitions",
];

/// CON-700, CON-702: a breadcrumb never carries a filter, a mode, a partition chip or a state; the title is its last step.
#[tokio::test]
async fn no_breadcrumb_carries_a_query_value_a_mode_or_a_state_and_the_title_is_its_last_step() {
    for path in PAGES {
        let (status, doc) = page(path).await;
        let steps = crumbs(&doc);
        assert!(!steps.is_empty(), "{path}: no breadcrumb");
        let query = path.split_once('?').map_or("", |q| q.1);
        let values: Vec<String> = query
            .split('&')
            .filter_map(|kv| kv.split_once('=').map(|x| x.1.to_lowercase()))
            .filter(|v| !v.is_empty())
            .collect();
        for (label, _) in &steps {
            let l = label.to_lowercase();
            assert!(
                !values.contains(&l),
                "{path}: the crumb {label:?} is a query value"
            );
            assert!(
                !MODES.contains(&l.as_str()),
                "{path}: the crumb {label:?} is a mode or a state"
            );
        }
        let last = &steps.last().unwrap().0;
        assert!(
            steps.last().unwrap().1.is_none(),
            "{path}: the last step is not a link"
        );
        assert_eq!(
            &title(&doc),
            last,
            "{path}: the tab says what the breadcrumb says ({status})"
        );
    }
}

/// CON-701: every link of a breadcrumb is a page that answers.
#[tokio::test]
async fn every_link_of_every_breadcrumb_is_a_page_that_answers() {
    let mut checked = std::collections::BTreeSet::new();
    for path in PAGES {
        let (_, doc) = page(path).await;
        for (label, href) in crumbs(&doc) {
            let Some(href) = href else { continue };
            if !checked.insert(href.clone()) {
                continue;
            }
            let (status, body) = page(&href).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "{path}: the step {label:?} links to {href}"
            );
            assert!(!title(&body).is_empty());
        }
    }
    assert!(
        checked.contains("/topics")
            && checked.contains("/groups")
            && checked.contains("/topics/orders")
    );
}

/// CON-703, CON-704: a screen this edition lacks is one calm page with its own breadcrumb, a way back, no Home content.
#[tokio::test]
async fn an_unbuilt_screen_is_one_designed_page_and_never_home_under_another_title() {
    let (_, home) = page("/").await;
    let home_extras: Vec<&str> = home
        .split("data-extra=\"")
        .skip(1)
        .map(|s| &s[..s.find('"').unwrap()])
        .collect();
    assert!(!home_extras.is_empty());
    for (path, crumb, name) in [
        ("/lab", "Lab", "Lab"),
        ("/access", "Access", "Access"),
        ("/settings", "Settings", "Settings"),
        ("/topics/orders/log", "Log", "Log"),
        ("/topics/orders/producers", "Producers", "Producers"),
        ("/groups/fulfilment/story", "Story", "Story"),
    ] {
        let (status, doc) = page(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(doc.contains("Not available in this edition"), "{path}");
        assert!(
            doc.contains("<meta name=\"robots\" content=\"noindex\">"),
            "{path}"
        );
        assert_eq!(crumbs(&doc).last().unwrap().0, crumb, "{path}");
        assert_eq!(title(&doc), crumb, "{path}");
        assert!(
            doc.contains(&format!("<h1>{name}</h1>")),
            "{path}: its own heading, not Home's"
        );
        for extra in &home_extras {
            assert!(
                !doc.contains(&format!("data-extra=\"{extra}\"")),
                "{path}: Home's {extra} on another page"
            );
        }
        assert!(!doc.contains("Connect an app"), "{path}");
    }
}

/// The way back is the page the visitor came from when it is one of ours, else the section.
#[tokio::test]
async fn the_unbuilt_page_goes_back_to_where_the_visitor_came_from_or_to_the_section() {
    let (router, _) = app("", &[]);
    let from = |referer: Option<&str>, path: &str| {
        let mut req = get(path);
        if let Some(r) = referer {
            req.headers_mut()
                .insert(header::REFERER, r.parse().unwrap());
        }
        req
    };
    let (_, own) = send(
        &router,
        from(
            Some("http://127.0.0.1:9093/topics/orders/messages?partition=1"),
            "/topics/orders/log",
        ),
    )
    .await;
    assert!(
        own.contains("<a class=\"btn pri\" href=\"/topics/orders/messages?partition=1\">")
            && own.contains("Go back"),
        "{own:.200}"
    );
    let (_, foreign) = send(
        &router,
        from(Some("http://evil.test/x"), "/topics/orders/log"),
    )
    .await;
    assert!(foreign.contains("href=\"/topics/orders\"") && foreign.contains("Back to the topic"));
    let (_, none) = send(&router, from(None, "/lab")).await;
    assert!(none.contains("Back to Home"));
}

/// The head and the tabs of a topic, as markup, with the one that is showing made equal.
fn frame(doc: &str) -> String {
    let head = between(doc, "<div class=\"page-h\">", "<div class=\"tabs\">");
    let tabs = between(doc, "<div class=\"tabs\">", "</div>");
    format!("{head}{tabs}")
        .replace("tab on\"", "tab\"")
        .replace(" aria-current=\"page\"", "")
}

/// CON-706: Messages, Config and Consumers start with the same head and the same tabs, byte for byte but the tab that is on.
#[tokio::test]
async fn the_tabs_of_a_topic_share_one_header() {
    let (_, messages) = page("/topics/orders").await;
    let (_, config) = page("/topics/orders/config").await;
    let (_, consumers) = page("/topics/orders/consumers").await;
    let (_, filtered) = page("/topics/orders/messages?partition=1&isolation=read_committed").await;
    let m = frame(&messages);
    assert_eq!(m, frame(&config));
    assert_eq!(m, frame(&consumers));
    assert_eq!(m, frame(&filtered));
    assert!(
        m.contains("Consumers<span class=\"n\">2</span>")
            && m.contains("href=\"/topics/orders/consumers\"")
    );
}

/// The controls of the Messages toolbar, in order: the names and labels a person would read off it.
fn controls(doc: &str) -> Vec<String> {
    let row = between(doc, "<div class=\"bar-row\">", "<input type=\"hidden\"");
    let mut out = Vec::new();
    for part in row.split('<').skip(1) {
        if part.starts_with("button") {
            out.push(part[part.find('>').unwrap() + 1..].trim_end().to_owned());
        } else if let Some(at) = part.find("name=\"") {
            let name = &part[at + 6..];
            out.push(name[..name.find('"').unwrap()].to_owned());
        }
    }
    out
}

/// CON-707: the toolbar has one layout: the same controls in the same order in every state, "Reading as" last, and the
/// transaction callout above it in every state of a topic that has an open transaction.
#[tokio::test]
async fn the_messages_toolbar_is_one_layout_and_the_callout_stays_in_every_state() {
    let states = [
        "/topics/orders",
        "/topics/orders/messages?partition=0",
        "/topics/orders/messages?partition=1&offset=16050",
        "/topics/orders/messages?key=ord-88412",
        "/topics/orders/messages?isolation=read_committed",
        "/topics/orders/messages?partition=0&isolation=read_committed",
    ];
    let mut first: Option<Vec<String>> = None;
    for path in states {
        let (status, doc) = page(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let c = controls(&doc);
        match &first {
            None => first = Some(c),
            Some(f) => {
                // The fields that carry a value differ in value only; the controls are the same list.
                assert_eq!(&c, f, "{path}: the toolbar is a different toolbar");
            }
        }
        let callout = doc
            .find("Transaction orders-eos is open.")
            .unwrap_or_else(|| panic!("{path}: the callout is gone"));
        let toolbar = doc.find("<form class=\"toolbar bar\"").unwrap();
        let table = doc.find("id=\"topic-messages\"").unwrap();
        assert!(
            callout < toolbar && toolbar < table,
            "{path}: callout, toolbar, data in that order"
        );
        let reading = doc.find(">Reading as<").unwrap();
        assert!(
            doc[reading..].find(">read_uncommitted<").unwrap()
                < doc[reading..].find(">read_committed<").unwrap()
        );
        assert!(
            reading > doc.find("All partitions").unwrap(),
            "{path}: Reading as is at the right"
        );
    }
    let f = first.unwrap();
    assert_eq!(f.last().map(String::as_str), Some("read_committed"));
    assert!(
        f.contains(&"All partitions".to_owned())
            && f.contains(&"find".to_owned())
            && f.contains(&"time".to_owned())
    );
}

/// CON-707: the second row of the toolbar names the filters that are on, each with a link that takes it off.
#[tokio::test]
async fn the_filters_that_are_on_are_chips_that_can_be_taken_off_and_none_is_a_sentence() {
    let (_, plain) = page("/topics/orders").await;
    assert!(plain.contains("No filters: the newest records of every partition."));
    let (_, on) =
        page("/topics/orders/messages?partition=1&offset=16050&isolation=read_committed").await;
    assert!(on.contains(">Partition 1<") || on.contains("Partition 1<svg"));
    assert!(
        on.contains("href=\"/topics/orders/messages?offset=16050&amp;isolation=read_committed\""),
        "{on:.100}"
    );
    assert!(on.contains("Clear filters"));
    assert!(
        on.contains("href=\"/topics/orders/messages?isolation=read_committed\""),
        "clearing keeps the isolation level"
    );
}

/// CON-708: no expander of any page is a link with a query.
#[tokio::test]
async fn no_page_has_an_expander_that_is_a_link_with_a_query() {
    for path in PAGES {
        let (_, doc) = page(path).await;
        for link in support::local_links(&doc) {
            assert!(
                !link.contains("open=") && !link.contains("replication=1"),
                "{path}: {link}"
            );
        }
        assert!(
            !doc.contains("aria-expanded"),
            "{path}: a disclosure is a <details>"
        );
    }
    let (_, deep) = page("/topics/orders/config?open=where:segment.bytes").await;
    assert!(deep.contains("id=\"where-segment.bytes\" open>"));
}
