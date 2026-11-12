//! The Groups pages: the list, a group, its members, its lag, and the dialogs that reset or delete its offsets or the group.
//! Every page is a plain GET whose choices are in the address; every write is a POST that answers the board's result state, or
//! redirects to the page that now shows the change. A group's name is hostile input: it is shown as text and put in links only
//! through `segment`.

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use maud::Markup;

use super::html;
use super::overlays::Parent;
use super::topics::page_of;
use crate::base::segment;
use crate::form::{get, parse};
use crate::model::{GroupFilter, GroupLagView};
use crate::pages::groups as page;
use crate::state::Shared;
use crate::stream_groups::{GROUP_IDS, GROUPS_IDS, LAG_IDS};
use crate::ui::shell::Rail;

pub(super) type Steps<'a> = Vec<(&'a str, Option<&'a str>)>;

pub(super) const RESOURCES: (&str, Option<&str>) = ("Resources", Some("/topics"));
pub(super) const GROUPS: (&str, Option<&str>) = ("Groups", Some("/groups"));

pub(super) async fn shell(
    s: &Shared,
    status: StatusCode,
    title: &str,
    steps: &[(&str, Option<&str>)],
    watch: &[&'static str],
    main: Markup,
) -> Response {
    let doc = s.document(Rail::Resources, title, steps, watch, main).await;
    (status, html(&doc)).into_response()
}

pub(super) async fn gone(s: &Shared, id: &str) -> Response {
    let shown: String = id.chars().take(60).collect();
    shell(
        s,
        StatusCode::NOT_FOUND,
        "No such group",
        &[RESOURCES, GROUPS, (&shown, None)],
        &[],
        page::no_group(id, &s.base),
    )
    .await
}

/// `/groups?q=..&state=..`
pub async fn groups(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let f = parse(query.as_deref().unwrap_or(""));
    let filter = get(&f, "q").unwrap_or("").trim().to_owned();
    let state = GroupFilter::parse(get(&f, "state").unwrap_or(""));
    let view = s.source.groups(&filter, state, get(&f, "hint")).await;
    let title = if view.total == 0 {
        "No groups yet"
    } else {
        "Groups"
    };
    let main = page::groups_page(&view, &s.base, &s.source.listen());
    shell(
        &s,
        StatusCode::OK,
        title,
        &[RESOURCES, ("Groups", None)],
        s.source.watching(GROUPS_IDS),
        main,
    )
    .await
}

/// The breadcrumb of a group's page, or of a page under it: Resources, Groups, the group, and the page (no link on the last).
fn trail(id: &str, last: Option<&str>) -> Vec<(String, Option<String>)> {
    let path = format!("/groups/{}", segment(id));
    let shown: String = id.chars().take(60).collect();
    let mut out = vec![
        ("Resources".to_owned(), Some("/topics".to_owned())),
        ("Groups".to_owned(), Some("/groups".to_owned())),
    ];
    match last {
        None => out.push((shown, None)),
        Some(label) => {
            out.push((shown, Some(path)));
            out.push((label.to_owned(), None));
        }
    }
    out
}

/// A group's own page as the parent of an overlay (the Delete-group dialog).
pub(super) fn group_parent(s: &Shared, view: &GroupLagView) -> Parent {
    Parent {
        rail: Rail::Resources,
        title: view.head.id.clone(),
        crumbs: trail(&view.head.id, None),
        watch: s.source.watching(GROUP_IDS),
        main: page::group_page(view, &s.base),
    }
}

/// `/groups/{group}`: classic, KIP-848 or rebalancing, by the group's data.
pub async fn group(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(view) = s.source.group(&id, false).await else {
        return gone(&s, &id).await;
    };
    html(&page_of(&s, group_parent(&s, &view)).await)
}

/// The members page as the parent of an overlay (a member's drawer).
pub(super) fn members_parent(s: &Shared, view: &GroupLagView) -> Parent {
    Parent {
        rail: Rail::Resources,
        title: "Members".to_owned(),
        crumbs: trail(&view.head.id, Some("Members")),
        watch: s.source.watching(GROUP_IDS),
        main: page::members_page(view, &s.base),
    }
}

/// `/groups/{group}/members`
pub async fn members(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(view) = s.source.group(&id, false).await else {
        return gone(&s, &id).await;
    };
    html(&page_of(&s, members_parent(&s, &view)).await)
}

/// The Lag page as the parent of an overlay (Reset offsets, Delete offsets); `flash` is what a redirect after a write says.
pub(super) fn lag_parent(s: &Shared, mut view: GroupLagView, flash: Option<String>) -> Parent {
    view.flash = flash;
    let title = if view.state == crate::model::LagState::NoCommits {
        "Lag: no commits yet"
    } else {
        "Lag"
    };
    Parent {
        rail: Rail::Resources,
        title: title.to_owned(),
        crumbs: trail(&view.head.id, Some(title)),
        watch: s.source.watching(LAG_IDS),
        main: page::lag_page(&view, &s.base),
    }
}

/// `/groups/{group}/lag?expiring=1&done=...`
pub async fn lag(
    State(s): State<Shared>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let f = parse(query.as_deref().unwrap_or(""));
    let Some(view) = s.source.group(&id, get(&f, "expiring") == Some("1")).await else {
        return gone(&s, &id).await;
    };
    let parent = lag_parent(&s, view, flash(get(&f, "done")));
    html(&page_of(&s, parent).await)
}

/// What a redirect after a write says on the page it lands on: only these words, never the address's own.
fn flash(done: Option<&str>) -> Option<String> {
    let (kind, n) = done?.split_once('-')?;
    let n: usize = n.parse().ok()?;
    let things = if n == 1 { "offset" } else { "offsets" };
    match kind {
        "reset" => Some(format!(
            "Reset {n} {things}. The group reads from them the next time a member starts."
        )),
        "forgot" => Some(format!("Deleted {n} committed {things}.")),
        _ => None,
    }
}
