//! The eleven boards of slice 9b that the reading half builds, rendered from the fixtures as whole documents.

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures;
use crate::fixtures_topics as world;
use crate::model::{MessageQuery, Reading};
use crate::pages::topics as page;
use crate::ui::shell::{Crumb, Page, Rail, document};

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
type Content = (
    maud::Markup,
    &'static str,
    Vec<(&'static str, Option<&'static str>)>,
);

const HOME: (&str, Option<&str>) = ("Home", Some("/"));
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
        partition: Some(0),
        offset: Some(16_098),
        ..MessageQuery::default()
    };
    let committed = MessageQuery {
        reading: Reading::Committed,
        ..MessageQuery::default()
    };
    Some(match id {
        "topic-detail" => (
            messages("orders", MessageQuery::default(), base)?,
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
        "topic-record" => (
            page::record_page(&world::record("orders", 1, 16_050)?, base),
            "orders-1 offset 16,050",
            orders("orders-1 offset 16,050"),
        ),
        "topic-record-binary" => (
            page::record_page(&world::record("orders", 2, 16_056)?, base),
            "orders-2 offset 16,056",
            orders("orders-2 offset 16,056"),
        ),
        _ => return None,
    })
}

/// The page of a board as a full document with no broker; `None` for an id that is not one of these.
#[must_use]
pub fn render(id: &str, base: &Base) -> Option<String> {
    let (main, title, steps) = list(id, base).or_else(|| topic(id, base))?;
    let assets = Assets::build(base);
    let chrome = fixtures::chrome();
    let page = Page {
        base,
        assets: &assets.urls,
        chrome: &chrome,
        active: Rail::Resources,
        crumbs: crumbs(base, &steps),
        title,
        watch: &[],
    };
    Some(document(&page, main).into_string())
}
