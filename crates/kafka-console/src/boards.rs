//! The five boards of slice 9a rendered from the "larder" fixtures, as whole documents: what `tests/boards.rs` writes to
//! `target/board-diff/<id>.html` and `tools/board-diff.ts` compares with the boards' signatures.

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures;
use crate::model::HomeState;
use crate::pages;
use crate::stream::WATCHABLE;
use crate::ui::shell::{Crumb, Page, Rail, document};

/// The breadcrumb of a board: label and path.
type Steps<'a> = Vec<(&'a str, Option<&'a str>)>;

/// The boards this slice builds, by screen id.
pub const IDS: [&str; 5] = [
    "home",
    "home-quiet",
    "home-attention",
    "connect",
    "connect-advertised",
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

/// The page of a board, as a full document, with no broker; `None` for an id that is not a board of this slice.
#[must_use]
pub fn render(id: &str, base: &Base) -> Option<String> {
    let assets = Assets::build(base);
    let chrome = fixtures::chrome();
    let (title, main, steps, watch): (&str, _, Steps, &[&'static str]) = match id {
        "home" | "home-quiet" | "home-attention" => {
            let state = match id {
                "home-quiet" => HomeState::Quiet,
                "home-attention" => HomeState::Attention(fixtures::attention()),
                _ => HomeState::Steady,
            };
            let view = fixtures::home(state);
            let title = crate::words::home_title(&view.state);
            (
                title,
                pages::home(&view, base),
                vec![("Home", None)],
                WATCHABLE,
            )
        }
        "connect" => (
            "Connect an app",
            pages::connect(&fixtures::connect(), base),
            vec![("Home", Some("/")), ("Connect an app", None)],
            &[],
        ),
        "connect-advertised" => (
            "Advertised listener",
            pages::advertised(&fixtures::probe_ok(), base),
            vec![
                ("Home", Some("/")),
                ("Connect", Some("/connect")),
                ("Advertised listener", None),
            ],
            &[],
        ),
        _ => return None,
    };
    let page = Page {
        base,
        assets: &assets.urls,
        chrome: &chrome,
        active: Rail::Home,
        crumbs: crumbs(base, &steps),
        title,
        watch,
    };
    Some(document(&page, main).into_string())
}
