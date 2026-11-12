//! The seven boards of the Log and Producers pages rendered from the larder with no broker, as whole documents: what
//! `tests/boards_log.rs` writes to `target/board-diff/<id>.html` for `tools/board-diff.ts`.

use maud::Markup;

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures;
use crate::fixtures_log as larder;
use crate::pages;
use crate::ui::shell::{Crumb, Page, Rail, document};

/// The boards this builder renders, by screen id.
pub const IDS: [&str; 7] = [
    "topic-log",
    "log-planner",
    "log-retention",
    "log-empty",
    "topic-producers",
    "txn-open",
    "txn-holding",
];

/// A board: the topic, the tail of its breadcrumb (label and path), and what is in `<main>`.
type Drawn = (&'static str, Vec<(String, Option<String>)>, Markup);

/// A page with the topic's wire dock under it, as the handlers draw the Log and Producers pages of a topic the larder's wire knows.
fn docked(topic: &str, main: Markup, base: &Base) -> Markup {
    let (rows, window) = crate::fixtures_live::topic_wire(topic);
    let dock = pages::live::topic_dock(
        topic,
        &rows,
        &window,
        crate::fixtures_live::now(),
        base,
        false,
    );
    maud::html! { (main) (dock) }
}

fn log(topic: &'static str, base: &Base) -> Option<Drawn> {
    let v = larder::log(topic, 0)?;
    let head = crate::fixtures_topics::head(topic)?;
    let page = pages::log_page(&v, &head, base, fixtures::ADDRESS);
    Some((
        topic,
        vec![("Log".to_owned(), None)],
        docked(topic, page, base),
    ))
}

fn board(id: &str, base: &Base) -> Option<Drawn> {
    let producers = "/topics/orders/producers".to_owned();
    match id {
        "topic-log" => log("inventory.levels", base),
        "log-planner" => log("customer.profiles", base),
        "log-retention" => log("orders", base),
        "log-empty" => log("orders.dlq", base),
        "topic-producers" => {
            let v = larder::producers("orders", 0, Vec::new())?;
            Some((
                "orders",
                vec![("Producers".to_owned(), None)],
                docked(
                    "orders",
                    pages::producers_page(&v, &crate::fixtures_topics::head("orders")?, base),
                    base,
                ),
            ))
        }
        "txn-open" => {
            let v = larder::txn_view("orders", "orders-eos")?;
            Some((
                "orders",
                vec![
                    ("Producers".to_owned(), Some(producers)),
                    ("orders-eos".to_owned(), None),
                ],
                pages::txn_page(&v, &crate::fixtures_topics::head("orders")?, base),
            ))
        }
        "txn-holding" => {
            let v = larder::txn_view("orders", "orders-eos")?;
            let txn = "/topics/orders/transactions/orders-eos".to_owned();
            Some((
                "orders",
                vec![
                    ("Producers".to_owned(), Some(producers)),
                    ("orders-eos".to_owned(), Some(txn)),
                    ("What it holds back".to_owned(), None),
                ],
                pages::txn_held_page(&v, &crate::fixtures_topics::head("orders")?, base),
            ))
        }
        _ => None,
    }
}

/// The page of a board, as a full document; `None` for an id that is not one of these.
#[must_use]
pub fn render(id: &str, base: &Base) -> Option<String> {
    let (topic, tail, main) = board(id, base)?;
    let assets = Assets::build(base);
    let chrome = fixtures::chrome();
    let at = |label: &str, path: Option<&str>| Crumb {
        label: label.to_owned(),
        href: path.map(|p| base.url(p)),
    };
    let mut crumbs = vec![
        at("Resources", Some("/topics")),
        at("Topics", Some("/topics")),
        at(topic, Some(&format!("/topics/{topic}"))),
    ];
    crumbs.extend(tail.iter().map(|(l, p)| at(l, p.as_deref())));
    let title = tail.last().map_or(topic, |t| t.0.as_str());
    let page = Page {
        base,
        assets: &assets.urls,
        chrome: &chrome,
        active: Rail::Resources,
        crumbs,
        title,
        watch: &[],
    };
    Some(document(&page, main).into_string())
}
