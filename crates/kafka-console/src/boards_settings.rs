//! The four boards of the Settings page rendered from the larder with no broker, as whole documents: what
//! `tests/boards_settings.rs` writes to `target/board-diff/<id>.html` for `tools/board-diff.ts`.

use maud::Markup;

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures;
use crate::fixtures_settings as larder;
use crate::gather::settings_edit::SettingsFields;
use crate::model::SettingsResult;
use crate::pages;
use crate::ui::overlay::{Over, over};
use crate::ui::shell::{Crumb, Page, Rail, document_over};

/// The boards this builder renders, by screen id.
pub const IDS: [&str; 4] = [
    "settings-home",
    "broker-config-edit",
    "broker-config-refused",
    "broker-config-saved",
];

/// A board: the title of its page and what is in `<main>`.
type Drawn = (String, Over);

fn fields<'a>(key: &'a str, value: &'a str) -> SettingsFields<'a> {
    SettingsFields {
        key,
        was: key,
        scope: "all",
        value,
    }
}

fn board(id: &str, base: &Base) -> Option<Drawn> {
    let parent = || pages::settings_page(&larder::page(), base);
    let plain = |title: &str, main: Markup| Some((title.to_owned(), over(main, maud::html! {})));
    match id {
        "settings-home" => plain("Settings", parent()),
        "broker-config-edit" => {
            let v = larder::form("log.retention.ms", "all")?;
            let overlay = pages::settings_edit_dialog(&v, base);
            Some(("Settings".to_owned(), over(parent(), overlay)))
        }
        "broker-config-saved" => {
            let SettingsResult::Saved(s) =
                larder::save(fields("log.retention.ms", "259200000"), false)
            else {
                return None;
            };
            plain("Saved", pages::settings_saved(&s, base))
        }
        "broker-config-refused" => {
            let SettingsResult::Refused(r) = larder::save(fields("num.partitions", "3"), true)
            else {
                return None;
            };
            plain("Refused", pages::settings_refused(&r, base))
        }
        _ => None,
    }
}

fn crumbs(base: &Base, title: &str) -> Vec<Crumb> {
    let at = |label: &str, path: Option<&str>| Crumb {
        label: label.to_owned(),
        href: path.map(|p| base.url(p)),
    };
    if matches!(title, "Saved" | "Refused") {
        vec![
            at("Home", Some("/")),
            at("Settings", Some("/settings")),
            at(title, None),
        ]
    } else {
        vec![at("Home", Some("/")), at("Settings", None)]
    }
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
        active: Rail::Settings,
        crumbs: crumbs(base, &title),
        title: &title,
        watch: &[],
    };
    Some(document_over(&page, main).into_string())
}
