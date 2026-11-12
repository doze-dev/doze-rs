//! The fourteen boards of the topic writes rendered from the larder with no broker, as whole documents: what
//! `tests/boards_write.rs` writes to `target/board-diff/<id>.html` for `tools/board-diff.ts`.

use maud::Markup;

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures;
use crate::fixtures_write as larder;
use crate::gather::{AddResult, DeleteResult, RecordsResult};
use crate::model::{CreateResult, ProduceInput, ProduceResult, Step};
use crate::pages;
use crate::ui::shell::{Crumb, Page, Rail, document};

/// The boards this builder renders, by screen id.
pub use crate::actions::WRITE_BOARDS as IDS;

type Steps<'a> = Vec<(&'a str, Option<&'a str>)>;

/// The input of the Produce boards: the larder's order, key `ord-88420`, partition 2.
#[must_use]
pub fn order_input() -> ProduceInput {
    ProduceInput {
        partition: "2".to_owned(),
        key: "ord-88420".to_owned(),
        value: r#"{"order_id":"ord-88420","customer":"c-3092","items":[{"sku":"FV-TOM-V","qty":1}],"total":7.40}"#.to_owned(),
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        acks: "-1".to_owned(),
    }
}

fn keyless_input() -> ProduceInput {
    ProduceInput {
        value: r#"{"sku":"FV-TOM-V","on_hand":412}"#.to_owned(),
        acks: "-1".to_owned(),
        ..ProduceInput::default()
    }
}

fn create_steps(wanted: Step, base: &Base) -> (&'static str, Markup, Steps<'static>) {
    let view = larder::create_landing(larder::draft_board(), wanted);
    let crumbs = match wanted {
        Step::Name => vec![("Topics", Some("/topics")), ("Create topic", None)],
        Step::Config => vec![
            ("Topics", Some("/topics")),
            ("Create topic", Some("/topics/new")),
            ("Configuration", None),
        ],
        Step::Review => vec![
            ("Topics", Some("/topics")),
            ("Create topic", Some("/topics/new")),
            ("Review and create", None),
        ],
    };
    let title = match wanted {
        Step::Name => "Create topic",
        Step::Config => "Configuration",
        Step::Review => "Review and create",
    };
    (title, pages::create_page(&view, base), crumbs)
}

fn overlay(
    title: &'static str,
    main: Markup,
    tail: &'static [(&'static str, Option<&'static str>)],
) -> (&'static str, Markup, Steps<'static>) {
    let mut crumbs = vec![
        ("Topics", Some("/topics")),
        ("orders", Some("/topics/orders")),
    ];
    crumbs.extend_from_slice(tail);
    crumbs.push((title, None));
    (title, main, crumbs)
}

/// The page of a board, as a full document; `None` for an id that is not one of these.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn render(id: &str, base: &Base) -> Option<String> {
    let (title, main, steps): (&str, Markup, Steps) = match id {
        "topic-create" => create_steps(Step::Name, base),
        "topic-create-config" => create_steps(Step::Config, base),
        "topic-create-review" => create_steps(Step::Review, base),
        "topic-create-refused" => {
            let CreateResult::Refused(r) = larder::refused_board() else {
                return None;
            };
            (
                "Create refused",
                pages::create_refused(&r, base),
                vec![
                    ("Topics", Some("/topics")),
                    ("Create topic", Some("/topics/new")),
                    ("Create refused", None),
                ],
            )
        }
        "topic-created" => {
            let CreateResult::Created(c) = larder::create_topic(larder::draft_board(), false)
            else {
                return None;
            };
            (
                "Topic created",
                pages::created(&c, base),
                vec![
                    ("Topics", Some("/topics")),
                    ("Create topic", Some("/topics/new")),
                    ("Topic created", None),
                ],
            )
        }
        "topic-produce" => overlay(
            "Produce",
            pages::produce_dialog(&larder::produce_form("orders", &order_input())?, base, true),
            &[],
        ),
        "topic-produce-keyless" => {
            let ProduceResult::Refused(r) =
                larder::produce_send("inventory.levels", &keyless_input())
            else {
                return None;
            };
            (
                "Produce refused: no key",
                pages::produce_refused(&r, base),
                vec![
                    ("Topics", Some("/topics")),
                    ("inventory.levels", Some("/topics/inventory.levels")),
                    ("Produce", Some("/topics/inventory.levels/produce")),
                    ("Produce refused: no key", None),
                ],
            )
        }
        "topic-produced" => {
            let ProduceResult::Sent(p) = larder::produce_send("orders", &order_input()) else {
                return None;
            };
            (
                "Record sent",
                pages::produced(&p, base),
                vec![
                    ("Topics", Some("/topics")),
                    ("orders", Some("/topics/orders")),
                    ("Record sent", None),
                ],
            )
        }
        "topic-add-partitions" => overlay(
            "Add partitions",
            pages::add_partitions_dialog(&larder::add_form("orders", Some("5"))?, base, true),
            &[("Partitions", Some("/topics/orders/partitions"))],
        ),
        "topic-add-partitions-refused" => {
            let AddResult::Refused(v) = larder::add_partitions("orders", "2") else {
                return None;
            };
            overlay(
                "Add partitions refused",
                pages::add_partitions_dialog(&v, base, true),
                &[("Partitions", Some("/topics/orders/partitions"))],
            )
        }
        "topic-delete-records" => overlay(
            "Delete records",
            pages::delete_records_dialog(
                &larder::delete_records_form("orders", 0, Some("16000"))?,
                base,
                true,
            ),
            &[("Partitions", Some("/topics/orders/partitions"))],
        ),
        "topic-delete-records-refused" => {
            let RecordsResult::Refused(r) = larder::delete_records("inventory.levels", 0, "16000")
            else {
                return None;
            };
            (
                "Delete records refused",
                pages::delete_records_refused(&r, base),
                vec![
                    ("Topics", Some("/topics")),
                    ("inventory.levels", Some("/topics/inventory.levels")),
                    ("Partitions", Some("/topics/inventory.levels/partitions")),
                    ("Delete records refused", None),
                ],
            )
        }
        "topic-delete" => overlay(
            "Delete topic",
            pages::delete_dialog(&larder::delete_form("orders", "orders")?, None, base, true),
            &[],
        ),
        "topic-deleted" => {
            let DeleteResult::Done(d) = larder::delete_topic("orders", "orders") else {
                return None;
            };
            (
                "Topic deleted",
                pages::deleted_page(&d, base),
                vec![("Topics", Some("/topics")), ("Topic deleted", None)],
            )
        }
        _ => return None,
    };
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
        active: Rail::Resources,
        crumbs,
        title,
        watch: &[],
    };
    Some(document(&page, main).into_string())
}
