//! The eleven boards of slice 9b that the reading half builds, rendered from the fixtures as whole documents.

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures;
use crate::fixtures_topics as world;
use crate::model::{MessageQuery, Reading};
use crate::pages::topics as page;
use crate::ui::overlay::{Over, over};
use crate::ui::shell::{Crumb, Page, Rail, document_over};

/// The boards this half builds, by screen id.
pub const IDS: [&str; 11] = [
    "topics-list",
    "topics-none",
    "topics-filtered",
    "topic-detail",
    "topic-messages-empty",
    "topic-messages-filtered",
    "topic-readcommitted",
    "topic-record",
    "topic-record-binary",
    "topic-partitions",
    "topic-offline",
];

fn crumbs(base: &Base, steps: &[(&str, Option<&str>)]) -> Vec<Crumb> {
    steps
        .iter()
        .map(|(label, path)| Crumb {
            label: (*label).to_owned(),
            href: path.map(|p| base.url(p)),
        })
        .collect()
}

fn messages(name: &str, q: MessageQuery, base: &Base) -> Option<maud::Markup> {
    let view = world::messages(name, q, Vec::new())?;
    Some(page::messages_page(
        &view,
        base,
        world::listen(),
        &view.query.to_query(),
    ))
}

/// A board's main content, the title of its page, and its breadcrumb.
pub(super) type Content = (
    maud::Markup,
    &'static str,
    Vec<(&'static str, Option<&'static str>)>,
);

const HOME: (&str, Option<&str>) = ("Resources", Some("/topics"));
const TOPICS: (&str, Option<&str>) = ("Topics", Some("/topics"));

/// The list in its three states.
fn list(id: &str, base: &Base) -> Option<Content> {
    let listen = world::listen();
    let (view, query, title) = match id {
        "topics-list" => (world::topics("", false, None), "", "Topics"),
        "topics-none" => (world::topics("", false, Some("none")), "", "No topics yet"),
        "topics-filtered" => (
            world::topics("refunds", false, None),
            "?q=refunds",
            "No topic matches",
        ),
        _ => return None,
    };
    Some((
        page::list_page(&view, base, listen, query),
        title,
        vec![HOME, ("Topics", None)],
    ))
}

/// The topic page with its wire dock under it (slice 9e), as the Messages handler draws it.
fn detail_with_dock(base: &Base) -> Option<maud::Markup> {
    let page = messages("orders", MessageQuery::default(), base)?;
    let (rows, w) = crate::fixtures_live::topic_wire("orders");
    let dock = crate::pages::live::topic_dock("orders", &rows, &w, base, false);
    Some(maud::html! { (page) (dock) })
}

/// The pages of `orders`, and of the two other topics of the world.
fn topic(id: &str, base: &Base) -> Option<Content> {
    let orders = |last: &'static str| {
        vec![
            HOME,
            TOPICS,
            ("orders", Some("/topics/orders")),
            (last, None),
        ]
    };
    let filtered = MessageQuery {
        partitions: vec![0],
        offset: Some(16_098),
        ..MessageQuery::default()
    };
    let committed = MessageQuery {
        reading: Reading::Committed,
        ..MessageQuery::default()
    };
    Some(match id {
        "topic-detail" => (
            detail_with_dock(base)?,
            "orders",
            vec![HOME, TOPICS, ("orders", None)],
        ),
        "topic-messages-empty" => (
            messages("orders.dlq", MessageQuery::default(), base)?,
            "orders.dlq",
            vec![HOME, TOPICS, ("orders.dlq", None)],
        ),
        "topic-messages-filtered" => (
            messages("orders", filtered, base)?,
            "orders-0 from 16,098",
            orders("orders-0 from 16,098"),
        ),
        "topic-readcommitted" => (
            messages("orders", committed, base)?,
            "read_committed",
            orders("read_committed"),
        ),
        "topic-offline" => (
            messages("notifications.email", MessageQuery::default(), base)?,
            "notifications.email partition 1 offline",
            vec![
                HOME,
                TOPICS,
                ("notifications.email", Some("/topics/notifications.email")),
                ("notifications.email partition 1 offline", None),
            ],
        ),
        "topic-partitions" => (
            page::partitions_page(&world::head("orders")?, base),
            "Partitions",
            orders("Partitions"),
        ),
        _ => return None,
    })
}

/// The drawer of a board: the record, over the Messages page of `orders`, which is that board's parent.
fn drawer(id: &str, base: &Base) -> Option<maud::Markup> {
    let (p, o) = match id {
        "topic-record" => (1, 16_050),
        "topic-record-binary" => (2, 16_056),
        _ => return None,
    };
    Some(page::record_drawer(
        &world::record("orders", p, o)?,
        base,
        "",
    ))
}

/// The page behind the overlays of the topic boards: the Messages page of `orders`, or its Partitions page.
#[must_use]
pub fn parent(partitions: bool, base: &Base) -> Option<Content> {
    topic(
        if partitions {
            "topic-partitions"
        } else {
            "topic-detail"
        },
        base,
    )
}

/// The page of a board as a full document with no broker; `None` for an id that is not one of these.
#[must_use]
pub fn render(id: &str, base: &Base) -> Option<String> {
    let (main, title, steps) = match drawer(id, base) {
        Some(_) => parent(false, base)?,
        None => list(id, base).or_else(|| topic(id, base))?,
    };
    let overlay = drawer(id, base).unwrap_or_default();
    document_for(base, title, &steps, over(main, overlay))
}

/// A document for a board: the shell, the page, and an overlay over it if there is one.
#[must_use]
pub fn document_for(
    base: &Base,
    title: &str,
    steps: &[(&str, Option<&str>)],
    over: Over,
) -> Option<String> {
    let assets = Assets::build(base);
    let chrome = fixtures::chrome();
    let page = Page {
        base,
        assets: &assets.urls,
        chrome: &chrome,
        active: Rail::Resources,
        crumbs: crumbs(base, steps),
        title,
        regions: &[],
    };
    Some(document_over(&page, over).into_string())
}
