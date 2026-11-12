//! The twelve boards of slice 9e (Live, its drawers, the first request and the two docks) rendered from the fixtures as whole
//! documents: what `tests/boards_live.rs` writes to `target/board-diff/<id>.html`.

use maud::Markup;

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures;
use crate::fixtures_live as world;
use crate::live_ids::{GROUP_WIRE_IDS, LIVE_IDS, TOPIC_WIRE_IDS};
use crate::model::WireQuery;
use crate::pages::live as page;
use crate::ui::shell::{Crumb, Page, Rail, document};

/// The boards this slice builds, by screen id.
pub const IDS: [&str; 12] = [
    "live-wire",
    "live-paused",
    "live-quiet",
    "live-load",
    "request-detail",
    "request-refused",
    "request-fenced",
    "request-too-large",
    "live-clients",
    "connect-first-request",
    "topic-wire",
    "group-wire",
];

/// A board's main content, the title of its page, its rail place, its breadcrumb and the fragments it watches.
type Content = (
    Markup,
    String,
    Rail,
    Vec<(&'static str, Option<&'static str>)>,
    &'static [&'static str],
);

fn live_page(hint: &str, base: &Base) -> Markup {
    page::live_page(&world::live(&WireQuery::default(), Some(hint)), base)
}

fn drawer(which: &str, base: &Base) -> Option<Markup> {
    let (d, limits) = world::request(which)?;
    let close = base.url("/live");
    let id = page::detail_heading_id(&d);
    let body = page::request_body(&d, &limits, base, fixtures::ADDRESS, &close);
    Some(page::drawer_page(live_page("steady", base), body, id))
}

#[allow(clippy::too_many_lines)] // one arm per board
fn content(id: &str, base: &Base) -> Option<Content> {
    let live = |hint: &str| {
        (
            live_page(hint, base),
            "Live".to_owned(),
            Rail::Live,
            vec![("Live", None)],
            LIVE_IDS,
        )
    };
    let over = |which: &str, last: &'static str| {
        Some((
            drawer(which, base)?,
            "Live".to_owned(),
            Rail::Live,
            vec![("Live", Some("/live")), (last, None)],
            LIVE_IDS,
        ))
    };
    Some(match id {
        "live-wire" => live("steady"),
        "live-paused" => live("paused"),
        "live-quiet" => live("quiet"),
        "live-load" => live("load"),
        "request-detail" => over("detail", "Fetch v12")?,
        "request-refused" => over("refused", "INVALID_RECORD")?,
        "request-fenced" => over("fenced", "INVALID_PRODUCER_EPOCH")?,
        "request-too-large" => over("too-large", "MESSAGE_TOO_LARGE")?,
        "live-clients" => {
            let body = page::clients_body(&world::clients(), base, &base.url("/live"));
            (
                page::drawer_page(live_page("steady", base), body, "live-clients"),
                "Live".to_owned(),
                Rail::Live,
                vec![("Live", Some("/live")), ("Clients", None)],
                LIVE_IDS,
            )
        }
        "connect-first-request" => (
            page::first_page(&world::first(), base),
            "Home".to_owned(),
            Rail::Home,
            vec![("Home", None)],
            &[],
        ),
        "topic-wire" => {
            let head = crate::fixtures_topics::head("orders")?;
            let (rows, w) = world::topic_wire("orders");
            let dock = page::topic_dock("orders", &rows, &w, world::now(), base, true);
            (
                crate::pages::topics::topic_wire_page(&head, dock, base),
                "orders on the wire".to_owned(),
                Rail::Resources,
                vec![
                    ("Resources", Some("/topics")),
                    ("Topics", Some("/topics")),
                    ("orders", Some("/topics/orders")),
                    ("orders on the wire", None),
                ],
                TOPIC_WIRE_IDS,
            )
        }
        "group-wire" => {
            let v = world::group_wire(&WireQuery {
                group: "fulfilment".to_owned(),
                ..WireQuery::default()
            });
            (
                page::group_wire_page(&v, base),
                "Wire".to_owned(),
                Rail::Resources,
                vec![
                    ("Resources", Some("/topics")),
                    ("Groups", Some("/groups")),
                    ("fulfilment", Some("/groups/fulfilment")),
                    ("Wire", None),
                ],
                GROUP_WIRE_IDS,
            )
        }
        _ => return None,
    })
}

/// The page of a board as a full document with no broker; `None` for an id that is not one of these.
#[must_use]
pub fn render(id: &str, base: &Base) -> Option<String> {
    let (main, title, active, steps, watch) = content(id, base)?;
    let assets = Assets::build(base);
    let chrome = fixtures::chrome();
    let crumbs = steps
        .iter()
        .map(|(label, path)| Crumb {
            label: (*label).to_owned(),
            href: path.map(|p| base.url(p)),
        })
        .collect();
    let page = Page {
        base,
        assets: &assets.urls,
        chrome: &chrome,
        active,
        crumbs,
        title: &title,
        watch,
    };
    Some(document(&page, main).into_string())
}
