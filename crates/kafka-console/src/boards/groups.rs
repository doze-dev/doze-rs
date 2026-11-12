//! The sixteen boards of slice 9c rendered from the "larder" group fixtures as whole documents: what `tests/boards_groups.rs`
//! writes to `target/board-diff/<id>.html` for `tools/board-diff.ts`.

use maud::Markup;

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures;
use crate::fixtures_groups as world;
use crate::model::GroupFilter;
use crate::pages::groups as page;
use crate::ui::shell::{Crumb, Page, Rail, document};

/// The boards this slice builds, by screen id.
pub const IDS: [&str; 16] = [
    "groups-list",
    "groups-none",
    "group-detail",
    "group-detail-848",
    "group-members",
    "member-detail",
    "group-rebalancing",
    "group-lag",
    "lag-no-commits",
    "lag-ahead",
    "lag-expiring",
    "group-reset-offsets",
    "group-reset-refused",
    "group-delete",
    "group-delete-refused",
    "group-offset-delete",
];

type Content = (Markup, String, Vec<(String, Option<String>)>);

fn steps(items: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
    items
        .iter()
        .map(|(l, p)| ((*l).to_owned(), p.map(str::to_owned)))
        .collect()
}

/// The breadcrumb of a page under a group: Resources, Groups, the group, and the page.
fn under(id: &str, last: &str) -> Vec<(String, Option<String>)> {
    let path = format!("/groups/{}", crate::base::segment(id));
    steps(&[
        ("Resources", Some("/topics")),
        ("Groups", Some("/groups")),
        (id, Some(&path)),
        (last, None),
    ])
}

fn at(id: &str) -> Vec<(String, Option<String>)> {
    steps(&[
        ("Resources", Some("/topics")),
        ("Groups", Some("/groups")),
        (id, None),
    ])
}

fn content(id: &str, base: &Base) -> Option<Content> {
    let group = |g: &str, expiring: bool| world::group(g, expiring);
    Some(match id {
        "groups-list" => (
            page::groups_page(
                &world::groups("", GroupFilter::All, None),
                base,
                fixtures::ADDRESS,
            ),
            "Groups".to_owned(),
            steps(&[("Resources", Some("/topics")), ("Groups", None)]),
        ),
        "group-detail" => (
            page::group_page(&group("search-indexer", false)?, base),
            "search-indexer".to_owned(),
            at("search-indexer"),
        ),
        "group-detail-848" => (
            page::group_page(&group("payments-workers", false)?, base),
            "payments-workers".to_owned(),
            at("payments-workers"),
        ),
        "group-rebalancing" => (
            page::group_page(&group("fulfilment", false)?, base),
            "fulfilment".to_owned(),
            at("fulfilment"),
        ),
        "group-members" => (
            page::members_page(&group("search-indexer", false)?, base),
            "Members".to_owned(),
            under("search-indexer", "Members"),
        ),
        "member-detail" => {
            let v = group("search-indexer", false)?;
            let m = v.head.members.first()?.clone();
            (
                page::member_drawer(&v.head, &m, base),
                m.id.clone(),
                under("search-indexer", &m.id),
            )
        }
        "group-lag" => (
            page::lag_page(&group("search-indexer", false)?, base),
            "Lag".to_owned(),
            under("search-indexer", "Lag"),
        ),
        "lag-no-commits" => (
            page::lag_page(&group("fulfilment", false)?, base),
            "Lag: no commits yet".to_owned(),
            under("fulfilment", "Lag: no commits yet"),
        ),
        "lag-ahead" => (
            page::lag_page(&group("analytics", false)?, base),
            "Lag".to_owned(),
            under("analytics", "Lag"),
        ),
        "lag-expiring" => (
            page::lag_page(&group("analytics", true)?, base),
            "Lag".to_owned(),
            under("analytics", "Lag"),
        ),
        _ => return overlays(id, base),
    })
}

/// The dialogs, the drawer and the refusals.
fn overlays(id: &str, base: &Base) -> Option<Content> {
    Some(match id {
        "groups-none" => (
            page::groups_page(
                &world::groups("", GroupFilter::All, Some("none")),
                base,
                fixtures::ADDRESS,
            ),
            "No groups yet".to_owned(),
            steps(&[("Resources", Some("/topics")), ("Groups", None)]),
        ),
        "group-reset-offsets" => (
            page::reset_dialog(
                &world::reset("analytics", "orders", "latest", "")?,
                base,
                true,
            ),
            "Reset offsets".to_owned(),
            under("analytics", "Reset offsets"),
        ),
        "group-reset-refused" => (
            page::reset_refused_page(&world::reset_refused("search-indexer")?, base),
            "Reset refused".to_owned(),
            under("search-indexer", "Reset refused"),
        ),
        "group-delete" => (
            page::delete_dialog(&world::delete_form("search-indexer")?, base, true),
            "Delete group".to_owned(),
            under("search-indexer", "Delete group"),
        ),
        "group-delete-refused" => (
            page::delete_refused_page(&world::delete_refused("search-indexer")?, base),
            "Delete refused".to_owned(),
            under("search-indexer", "Delete refused"),
        ),
        "group-offset-delete" => (
            page::offset_delete_dialog(
                &world::offset_delete_form("analytics", &[])?,
                None,
                base,
                true,
            ),
            "Delete offsets".to_owned(),
            under("analytics", "Delete offsets"),
        ),
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
        watch: &[],
    };
    Some(document(&page, main).into_string())
}
