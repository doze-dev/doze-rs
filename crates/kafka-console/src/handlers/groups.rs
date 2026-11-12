//! The Groups pages: the list, a group, its members, its lag, and the dialogs that reset or delete its offsets or the group.
//! Every page is a plain GET whose choices are in the address; every write is a POST that answers the board's result state, or
//! redirects to the page that now shows the change. A group's name is hostile input: it is shown as text and put in links only
//! through `segment`.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;

use super::html;
use crate::base::segment;
use crate::form::{get, parse, parse_body, text};
use crate::gather::{DeleteGroupResult, OffsetDeleteResult, ResetResult};
use crate::model::{GroupFilter, GroupLagView};
use crate::pages::groups as page;
use crate::state::Shared;
use crate::stream_groups::{GROUP_IDS, GROUPS_IDS, LAG_IDS};
use crate::ui::client::HEADER_FRAGMENT;
use crate::ui::shell::Rail;

type Steps<'a> = Vec<(&'a str, Option<&'a str>)>;

const RESOURCES: (&str, Option<&str>) = ("Resources", Some("/topics"));
const GROUPS: (&str, Option<&str>) = ("Groups", Some("/groups"));

async fn shell(
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

async fn gone(s: &Shared, id: &str) -> Response {
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

/// `/groups/{group}`: classic, KIP-848 or rebalancing, by the group's data.
pub async fn group(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(view) = s.source.group(&id, false).await else {
        return gone(&s, &id).await;
    };
    let main = page::group_page(&view, &s.base);
    shell(
        &s,
        StatusCode::OK,
        &id,
        &[RESOURCES, GROUPS, (&id, None)],
        s.source.watching(GROUP_IDS),
        main,
    )
    .await
}

fn under<'a>(id: &'a str, path: &'a str, last: &'a str) -> Steps<'a> {
    vec![RESOURCES, GROUPS, (id, Some(path)), (last, None)]
}

/// `/groups/{group}/members`
pub async fn members(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(view) = s.source.group(&id, false).await else {
        return gone(&s, &id).await;
    };
    let path = format!("/groups/{}", segment(&id));
    let main = page::members_page(&view, &s.base);
    shell(
        &s,
        StatusCode::OK,
        "Members",
        &under(&id, &path, "Members"),
        s.source.watching(GROUP_IDS),
        main,
    )
    .await
}

/// `/groups/{group}/members/{member}`
pub async fn member(
    State(s): State<Shared>,
    Path((id, member)): Path<(String, String)>,
) -> Response {
    let Some(view) = s.source.group(&id, false).await else {
        return gone(&s, &id).await;
    };
    let Some(m) = view.head.members.iter().find(|m| m.id == member) else {
        return gone(&s, &id).await;
    };
    let path = format!("/groups/{}", segment(&id));
    let members = format!("{path}/members");
    let main = maud::html! { (page::parents::of_member(&view, &s.base)) (page::member_drawer(&view.head, m, &s.base)) };
    let shown: String = member.chars().take(60).collect();
    let steps: Steps = vec![
        RESOURCES,
        GROUPS,
        (&id, Some(&path)),
        ("Members", Some(&members)),
        (&shown, None),
    ];
    shell(&s, StatusCode::OK, &shown, &steps, &[], main).await
}

/// `/groups/{group}/lag?expiring=1&done=...`
pub async fn lag(
    State(s): State<Shared>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let f = parse(query.as_deref().unwrap_or(""));
    let Some(mut view) = s.source.group(&id, get(&f, "expiring") == Some("1")).await else {
        return gone(&s, &id).await;
    };
    view.flash = flash(get(&f, "done"));
    let path = format!("/groups/{}", segment(&id));
    let title = if view.state == crate::model::LagState::NoCommits {
        "Lag: no commits yet"
    } else {
        "Lag"
    };
    let main = page::lag_page(&view, &s.base);
    shell(
        &s,
        StatusCode::OK,
        title,
        &under(&id, &path, title),
        s.source.watching(LAG_IDS),
        main,
    )
    .await
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

fn wants_fragment(h: &HeaderMap) -> bool {
    h.contains_key(HEADER_FRAGMENT)
}

/// A dialog on a page of its own under the group's breadcrumb (or alone, for a script).
/// An overlay: alone for a script (which opens it over the page it is on), else its parent page with the overlay open.
/// Until the overlay system of `ui/overlay.rs` lands the overlay is drawn in the shell of `ui/dialog.rs`, after its parent.
async fn dialog(
    s: &Shared,
    status: StatusCode,
    headers: &HeaderMap,
    (id, title): (&str, &str),
    parent: fn(&GroupLagView, &crate::base::Base) -> Markup,
    make: impl Fn(bool) -> Markup,
) -> Response {
    if wants_fragment(headers) {
        return (status, html(&make(false))).into_response();
    }
    let path = format!("/groups/{}", segment(id));
    let main = match s.source.group(id, false).await {
        Some(v) => maud::html! { (parent(&v, &s.base)) (make(true)) },
        None => make(true),
    };
    shell(s, status, title, &under(id, &path, title), &[], main).await
}

/// `GET /groups/{group}/reset?topic=&to=&value=`
pub async fn reset_form(
    State(s): State<Shared>,
    Path(id): Path<String>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = parse(q.as_deref().unwrap_or(""));
    let Some(v) = s
        .source
        .reset_form(
            &id,
            text(&f, "topic"),
            get(&f, "to").unwrap_or("latest"),
            text(&f, "value"),
        )
        .await
    else {
        return gone(&s, &id).await;
    };
    dialog(
        &s,
        StatusCode::OK,
        &headers,
        (&id, "Reset offsets"),
        page::parents::of_offsets,
        |inline| page::reset_dialog(&v, &s.base, inline),
    )
    .await
}

fn lag_url(s: &Shared, id: &str, done: &str) -> String {
    s.base.url(&format!("/groups/{}/lag{done}", segment(id)))
}

/// `POST /groups/{group}/reset`
pub async fn reset(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let f = parse_body(&body);
    let to = get(&f, "to").unwrap_or("latest");
    match s
        .source
        .reset_offsets(&id, text(&f, "topic"), to, text(&f, "value"))
        .await
    {
        ResetResult::Done(n) => {
            Redirect::to(&lag_url(&s, &id, &format!("?done=reset-{n}"))).into_response()
        }
        ResetResult::Refused(r) => {
            let path = format!("/groups/{}", segment(&id));
            let lag = format!("{path}/lag");
            let steps: Steps = vec![
                RESOURCES,
                GROUPS,
                (&id, Some(&path)),
                ("Lag", Some(&lag)),
                ("Reset refused", None),
            ];
            shell(
                &s,
                StatusCode::OK,
                "Reset refused",
                &steps,
                &[],
                page::reset_refused_page(&r, &s.base),
            )
            .await
        }
        ResetResult::Invalid(v) => {
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                (&id, "Reset offsets"),
                page::parents::of_offsets,
                |inline| page::reset_dialog(&v, &s.base, inline),
            )
            .await
        }
        ResetResult::NoGroup => gone(&s, &id).await,
    }
}

/// `GET /groups/{group}/delete`
pub async fn delete_form(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(v) = s.source.delete_group_form(&id).await else {
        return gone(&s, &id).await;
    };
    dialog(
        &s,
        StatusCode::OK,
        &headers,
        (&id, "Delete group"),
        page::parents::of_group,
        |inline| page::delete_dialog(&v, &s.base, inline),
    )
    .await
}

/// `POST /groups/{group}/delete`
pub async fn delete(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    match s.source.delete_group(&id).await {
        DeleteGroupResult::Done => Redirect::to(&s.base.url("/groups")).into_response(),
        DeleteGroupResult::Refused(r) => {
            let path = format!("/groups/{}", segment(&id));
            let steps: Steps = vec![
                RESOURCES,
                GROUPS,
                (&id, Some(&path)),
                ("Delete refused", None),
            ];
            shell(
                &s,
                StatusCode::OK,
                "Delete refused",
                &steps,
                &[],
                page::delete_refused_page(&r, &s.base),
            )
            .await
        }
        DeleteGroupResult::NoGroup => gone(&s, &id).await,
    }
}

fn chosen(f: &[(String, String)]) -> Vec<String> {
    f.iter()
        .filter(|(k, _)| k == "topic")
        .map(|(_, v)| v.clone())
        .collect()
}

/// `GET /groups/{group}/offsets/delete?topic=..`
pub async fn offset_delete_form(
    State(s): State<Shared>,
    Path(id): Path<String>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = parse(q.as_deref().unwrap_or(""));
    let Some(v) = s.source.offset_delete_form(&id, &chosen(&f)).await else {
        return gone(&s, &id).await;
    };
    dialog(
        &s,
        StatusCode::OK,
        &headers,
        (&id, "Delete offsets"),
        page::parents::of_offsets,
        |inline| page::offset_delete_dialog(&v, None, &s.base, inline),
    )
    .await
}

/// `POST /groups/{group}/offsets/delete`
pub async fn offset_delete(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let f = parse_body(&body);
    match s.source.offset_delete(&id, &chosen(&f)).await {
        OffsetDeleteResult::Done(n) => {
            Redirect::to(&lag_url(&s, &id, &format!("?done=forgot-{n}"))).into_response()
        }
        OffsetDeleteResult::Refused(v, r) => {
            dialog(
                &s,
                StatusCode::OK,
                &headers,
                (&id, "Delete offsets refused"),
                page::parents::of_offsets,
                |inline| page::offset_delete_dialog(&v, Some(&r), &s.base, inline),
            )
            .await
        }
        OffsetDeleteResult::Nothing(v) => {
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                (&id, "Delete offsets"),
                page::parents::of_offsets,
                |inline| page::offset_delete_dialog(&v, None, &s.base, inline),
            )
            .await
        }
        OffsetDeleteResult::NoGroup => gone(&s, &id).await,
    }
}
