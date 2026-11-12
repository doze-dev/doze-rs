//! The overlays of a group: a member's drawer over the Members page, and the Reset, Delete-group and Delete-offsets dialogs
//! over the Lag page (the group's own page for Delete group). Each is a route that answers the overlay alone to a script
//! (`X-Fragment`) and its parent page with the overlay open on it to a browser; a write is a POST whose result is a page
//! (a redirect to the Lag page, a refusal drawn as a page) or the same dialog again (a field to fix, a refusal drawn in it).

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;

use super::groups::{
    GROUPS, RESOURCES, Steps, gone, group_parent, lag_parent, members_parent, shell,
};
use super::overlays;
use crate::base::segment;
use crate::form::{get, parse, parse_body, text};
use crate::gather::{DeleteGroupResult, OffsetDeleteResult, ResetResult};
use crate::pages::groups as page;
use crate::state::Shared;

/// The page an overlay of a group sits over.
#[derive(Clone, Copy)]
enum Under {
    /// The Lag page (Reset offsets).
    Lag,
    /// The Lag page with the offsets that are expiring (Delete offsets).
    Expiring,
    /// The group's own page (Delete group).
    Group,
}

/// An overlay of a group: alone for a script, over its parent page for a browser.
async fn overlay_over(
    s: &Shared,
    status: StatusCode,
    headers: &HeaderMap,
    (id, under): (&str, Under),
    overlay: Markup,
) -> Response {
    if overlays::wants_fragment(headers) {
        return overlays::fragment(status, &overlay);
    }
    let Some(view) = s.source.group(id, matches!(under, Under::Expiring)).await else {
        return gone(s, id).await;
    };
    let parent = match under {
        Under::Group => group_parent(s, &view),
        Under::Lag | Under::Expiring => lag_parent(s, view, None),
    };
    overlays::over_parent(s, status, parent, overlay).await
}

/// `/groups/{group}/members/{member}`: a drawer over the Members page.
pub async fn member(
    State(s): State<Shared>,
    Path((id, member)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let Some(view) = s.source.group(&id, false).await else {
        return gone(&s, &id).await;
    };
    let Some(m) = view.head.members.iter().find(|m| m.id == member) else {
        return gone(&s, &id).await;
    };
    let drawer = page::member_drawer(&view.head, m, &s.base);
    if overlays::wants_fragment(&headers) {
        return overlays::fragment(StatusCode::OK, &drawer);
    }
    overlays::over_parent(
        &s,
        StatusCode::OK,
        members_parent(&s, &view, Some(&member)),
        drawer,
    )
    .await
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
    let dialog = page::reset_dialog(&v, &s.base);
    overlay_over(&s, StatusCode::OK, &headers, (&id, Under::Lag), dialog).await
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
            let dialog = page::reset_dialog(&v, &s.base);
            let status = StatusCode::UNPROCESSABLE_ENTITY;
            overlay_over(&s, status, &headers, (&id, Under::Lag), dialog).await
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
    let dialog = page::delete_dialog(&v, &s.base);
    overlay_over(&s, StatusCode::OK, &headers, (&id, Under::Group), dialog).await
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
    let dialog = page::offset_delete_dialog(&v, None, &s.base);
    overlay_over(&s, StatusCode::OK, &headers, (&id, Under::Expiring), dialog).await
}

/// `POST /groups/{group}/offsets/delete`
pub async fn offset_delete(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let f = parse_body(&body);
    let at = (id.as_str(), Under::Expiring);
    match s.source.offset_delete(&id, &chosen(&f)).await {
        OffsetDeleteResult::Done(n) => {
            Redirect::to(&lag_url(&s, &id, &format!("?done=forgot-{n}"))).into_response()
        }
        OffsetDeleteResult::Refused(v, r) => {
            let dialog = page::offset_delete_dialog(&v, Some(&r), &s.base);
            overlay_over(&s, StatusCode::OK, &headers, at, dialog).await
        }
        OffsetDeleteResult::Nothing(v) => {
            let dialog = page::offset_delete_dialog(&v, None, &s.base);
            let status = StatusCode::UNPROCESSABLE_ENTITY;
            overlay_over(&s, status, &headers, at, dialog).await
        }
        OffsetDeleteResult::NoGroup => gone(&s, &id).await,
    }
}
