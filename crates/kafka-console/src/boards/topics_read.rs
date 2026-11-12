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
        .map(|(label, path)| Crumb { label: (*label).to_owned(), href: path.map(|p| base.url(p)) })
        .collect()
}

fn messages(name: &str, q: MessageQuery, base: &Base) -> Option<maud::Markup> {
    let view = world::messages(name, q, Vec::new())?;
    Some(page::messages_page(&view, base, world::listen(), &view.query.to_query()))
}

/// The main content and the breadcrumb of a board.
fn content(id: &str, base: &Base) -> Option<(maud::Markup, &'static str, Vec<(&'static str, Option<&'static str>)>)> {
    let home = ("Home", Some("/"));
    let topics = ("Topics", Some("/topics"));
    let listen = world::listen();
    let on_orders = |last: &'static str| vec![home, topics, ("orders", Some("/topics/orders")), (last, None)];
    Some(match id {
        "topics-list" => (page::list_page(&world::topics("", false, None), base, listen, ""), "Topics", vec![home, ("Topics", None)]),
        "topics-none" => (page::list_page(&world::topics("", false, Some("none")), base, listen, ""), "No topics yet", vec![home, ("Topics", None)]),
        "topics-filtered" => (page::list_page(&world::topics("refunds", false, None), base, listen, "?q=refunds"), "No topic matches", vec![home, ("Topics", None)]),
        "topic-detail" => (messages("orders", MessageQuery::default(), base)?, "orders", vec![home, topics, ("orders", None)]),
        "topic-messages-empty" => (messages("orders.dlq", MessageQuery::default(), base)?, "orders.dlq", vec![home, topics, ("orders.dlq", None)]),
        "topic-messages-filtered" => {
            let q = MessageQuery { partition: Some(0), offset: Some(16_098), ..MessageQuery::default() };
            (messages("orders", q, base)?, "orders-0 from 16,098", on_orders("orders-0 from 16,098"))
        }
        "topic-readcommitted" => {
            let q = MessageQuery { reading: Reading::Committed, ..MessageQuery::default() };
            (messages("orders", q, base)?, "read_committed", on_orders("read_committed"))
        }
        "topic-offline" => (
            messages("notifications.email", MessageQuery::default(), base)?,
            "notifications.email partition 1 offline",
            vec![home, topics, ("notifications.email", Some("/topics/notifications.email")), ("notifications.email partition 1 offline", None)],
        ),
        "topic-partitions" => (page::partitions_page(&world::head("orders")?, base), "Partitions", on_orders("Partitions")),
        "topic-record" => (page::record_page(&world::record("orders", 1, 16_050)?, base), "orders-1 offset 16,050", on_orders("orders-1 offset 16,050")),
        "topic-record-binary" => (page::record_page(&world::record("orders", 2, 16_056)?, base), "orders-2 offset 16,056", on_orders("orders-2 offset 16,056")),
        _ => return None,
    })
}

/// The page of a board as a full document with no broker; `None` for an id that is not one of these.
#[must_use]
pub fn render(id: &str, base: &Base) -> Option<String> {
    let (main, title, steps) = content(id, base)?;
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
