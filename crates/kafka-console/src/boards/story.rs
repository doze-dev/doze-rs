//! The three boards of slice 9f (the Story of a group) rendered from the larder's notes as whole documents: what
//! `tests/boards_story.rs` writes to `target/board-diff/<id>.html` for `tools/board-diff.ts`.

use maud::Markup;

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures;
use crate::fixtures_story::story;
use crate::pages::story as page;
use crate::ui::overlay::{Over, over};
use crate::ui::shell::{Crumb, Page, Rail, document_over};

/// The boards this slice builds, by screen id.
pub const IDS: [&str; 3] = ["group-story", "story-rebalance", "story-flapping"];

type Steps = Vec<(String, Option<String>)>;

fn steps(items: &[(&str, Option<&str>)]) -> Steps {
    items
        .iter()
        .map(|(l, p)| ((*l).to_owned(), p.map(str::to_owned)))
        .collect()
}

fn trail(last: Option<&str>) -> Steps {
    let mut s = steps(&[
        ("Resources", Some("/topics")),
        ("Groups", Some("/groups")),
        ("fulfilment", Some("/groups/fulfilment")),
    ]);
    match last {
        Some(_) => s.push((
            "Story".to_owned(),
            Some("/groups/fulfilment/story".to_owned()),
        )),
        None => s.push(("Story".to_owned(), None)),
    }
    s
}

fn content(id: &str, base: &Base) -> Option<(Over, String, Steps)> {
    let v = story("fulfilment")?;
    Some(match id {
        "group-story" => (
            over(page::story_page(&v, base), Markup::default()),
            "Story".to_owned(),
            trail(None),
        ),
        "story-rebalance" => {
            let g = v.generations.first()?;
            (
                over(
                    page::story_page(&v, base),
                    page::rebalance_drawer(&v, g, base),
                ),
                "Story".to_owned(),
                trail(None),
            )
        }
        "story-flapping" => {
            let mut crumbs = trail(Some("Story"));
            crumbs.push(("Flapping".to_owned(), None));
            (
                over(page::flapping_page(&v, base), Markup::default()),
                "Flapping".to_owned(),
                crumbs,
            )
        }
        _ => return None,
    })
}

/// The page of a board as a full document with no broker; `None` for an id that is not one of these.
#[must_use]
pub fn render(id: &str, base: &Base) -> Option<String> {
    let (main, title, steps) = content(id, base)?;
    let assets = Assets::build(base);
    let chrome = fixtures::chrome();
    let crumbs = steps
        .iter()
        .map(|(label, path)| Crumb {
            label: label.clone(),
            href: path.as_deref().map(|p| base.url(p)),
        })
        .collect();
    let page = Page {
        base,
        assets: &assets.urls,
        chrome: &chrome,
        active: Rail::Resources,
        crumbs,
        title: &title,
        regions: &[],
    };
    Some(document_over(&page, main).into_string())
}
