//! The eight boards of the Config pages rendered from the larder with no broker, as whole documents: what
//! `tests/boards_config.rs` writes to `target/board-diff/<id>.html` for `tools/board-diff.ts`.

use maud::{Markup, html};

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures;
use crate::fixtures_config::{self as larder, TOPIC};
use crate::gather::ConfigFields;
use crate::model::{ConfigAsk, ConfigResult};
use crate::pages;
use crate::ui::overlay::{Over, over};
use crate::ui::shell::{Crumb, Page, Rail, document_over};

/// The boards this builder renders, by screen id.
pub use crate::actions::config::BOARDS as IDS;

const PATH: &str = "/topics/inventory.levels";
const CONFIG: &str = "/topics/inventory.levels/config";

/// A board: the title of its page and what is in `<main>`, with the dialog over it when it is one.
type Drawn = (String, Over);

/// A page with no overlay.
fn plain(title: &str, main: Markup) -> Drawn {
    (title.to_owned(), over(main, html! {}))
}

fn fields<'a>(
    key: &'a str,
    op: Option<&'a str>,
    value: &'a str,
    other: Option<&'a str>,
) -> ConfigFields<'a> {
    ConfigFields {
        key,
        op,
        value: Some(value),
        other,
    }
}

fn saved(base: &Base) -> Option<Drawn> {
    let f = fields("min.cleanable.dirty.ratio", None, "0.7", None);
    let ConfigResult::Saved(s) = larder::save(TOPIC, f, false) else {
        return None;
    };
    Some(plain("Saved", pages::config_saved(&s, base)))
}

fn refused(base: &Base, f: ConfigFields<'_>) -> Option<Drawn> {
    let ConfigResult::Refused(r) = larder::save(TOPIC, f, false) else {
        return None;
    };
    Some(plain("Refused", pages::config_refused(&r, base)))
}

/// A dialog board: the Config page with the dialog over it, as a direct visit to the dialog's address shows it.
fn dialog_over(base: &Base, overlay: Markup) -> Option<Drawn> {
    let head = crate::fixtures_topics::head(TOPIC)?;
    let parent = pages::config_page(
        &larder::page(TOPIC, ConfigAsk::default())?,
        Some(&head),
        base,
    );
    Some(("Config".to_owned(), over(parent, overlay)))
}

fn dialog(base: &Base, f: ConfigFields<'_>) -> Option<Drawn> {
    let v = larder::form(TOPIC, f).ok()?;
    dialog_over(base, pages::config_edit_dialog(&v, base))
}

fn board(id: &str, base: &Base) -> Option<Drawn> {
    match id {
        "topic-config" => {
            let ask = ConfigAsk {
                open: vec![
                    pages::config_token("where", "segment.bytes"),
                    pages::config_token("text", "min.cleanable.dirty.ratio"),
                ],
                ..ConfigAsk::default()
            };
            let page = larder::page(TOPIC, ask)?;
            let head = crate::fixtures_topics::head(TOPIC)?;
            Some(plain(
                "Config",
                pages::config_page(&page, Some(&head), base),
            ))
        }
        "config-edit" => dialog(base, fields("min.cleanable.dirty.ratio", None, "0.7", None)),
        "config-edit-list" => dialog(
            base,
            fields("cleanup.policy", Some("append"), "compacted", None),
        ),
        "config-edit-lag" => dialog(
            base,
            fields("min.compaction.lag.ms", None, "7200000", Some("3600000")),
        ),
        "config-saved" => saved(base),
        "config-refused" => refused(
            base,
            fields("min.compaction.lag.ms", None, "7200000", Some("3600000")),
        ),
        "config-refused-value" => refused(
            base,
            fields("cleanup.policy", Some("append"), "compacted", None),
        ),
        "config-reset" => {
            let v = larder::reset_form(TOPIC, "segment.bytes").ok()?;
            dialog_over(base, pages::config_reset_dialog(&v, base))
        }
        _ => None,
    }
}

fn crumbs(base: &Base, title: &str) -> Vec<Crumb> {
    let at = |label: &str, path: Option<&str>| Crumb {
        label: label.to_owned(),
        href: path.map(|p| base.url(p)),
    };
    let mut out = vec![
        at("Resources", Some("/topics")),
        at("Topics", Some("/topics")),
        at(TOPIC, Some(PATH)),
    ];
    if title == "Config" {
        out.push(at("Config", None));
    } else {
        out.push(at("Config", Some(CONFIG)));
        out.push(at(title, None));
    }
    out
}

/// The page of a board, as a full document; `None` for an id that is not one of these.
#[must_use]
pub fn render(id: &str, base: &Base) -> Option<String> {
    let (title, main) = board(id, base)?;
    let assets = Assets::build(base);
    let chrome = fixtures::chrome();
    let page = Page {
        base,
        assets: &assets.urls,
        chrome: &chrome,
        active: Rail::Resources,
        crumbs: crumbs(base, &title),
        title: &title,
        watch: &[],
    };
    Some(document_over(&page, main).into_string())
}
