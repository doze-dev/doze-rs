//! The Access page and its two dialogs, Create ACL and Delete ACL, each with its POST. A dialog is an overlay (`ui::overlay`)
//! over the Access page: its route answers the dialog alone to a script (`X-Fragment`) and the Access page with the dialog open
//! on it to a browser without one. A GET never changes anything; every change is a POST with the same-origin proof, and a
//! success goes back to the page (303).

use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;

use super::html;
use super::overlays::{self, Parent};
use crate::form::{get, parse, parse_body, text};
use crate::gather::access_dialogs::{AclFields, entry_of, input_of, missing};
use crate::model::{AclCreateResult, AclDeleteResult, AclEntry};
use crate::pages;
use crate::state::Shared;
use crate::trail::{Place, borrowed};
use crate::ui::shell::Rail;

async fn shell(s: &Shared, status: StatusCode, main: Markup) -> Response {
    let steps = Place::Access.steps();
    let doc = s
        .document(Rail::Access, "Access", &borrowed(&steps), &[], main)
        .await;
    (status, html(&doc)).into_response()
}

/// The Access page as the parent of an overlay (the two ACL dialogs); `state` is the fixtures' `?state=` word, none for a live broker.
async fn access_parent(s: &Shared, state: Option<&str>) -> Parent {
    let view = s.source.access(state).await;
    Parent {
        rail: Rail::Access,
        title: "Access".to_owned(),
        crumbs: Place::Access.steps(),
        regions: &[],
        main: pages::access_page(&view, &s.base),
    }
}

/// A dialog: alone for a script (which opens it over the page it is on), else the Access page with the dialog open on it.
async fn dialog(s: &Shared, status: StatusCode, h: &HeaderMap, overlay: Markup) -> Response {
    if overlays::wants_fragment(h) {
        return overlays::fragment(status, &overlay);
    }
    overlays::over_parent(s, status, access_parent(s, None).await, overlay).await
}

/// The fields of a form or a query; one that is not there is empty.
fn fields(f: &[(String, String)]) -> AclFields<'_> {
    AclFields {
        kind: text(f, "type"),
        name: text(f, "name"),
        pattern: text(f, "pattern"),
        principal: text(f, "principal"),
        host: text(f, "host"),
        op: text(f, "op"),
        perm: text(f, "perm"),
    }
}

/// `GET /access`
pub async fn access(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    // `?state=secured` is read by the fixtures only; a live broker decides by its data.
    let f = parse(q.as_deref().unwrap_or(""));
    let parent = access_parent(&s, get(&f, "state")).await;
    html(&super::topics::page_of(&s, parent).await)
}

/// `GET /access/acls/new`
pub async fn acl_create_form(
    State(s): State<Shared>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = parse(q.as_deref().unwrap_or(""));
    let v = s.source.acl_create_form(&fields(&f)).await;
    dialog(
        &s,
        StatusCode::OK,
        &headers,
        pages::acl_create_dialog(&v, &s.base),
    )
    .await
}

/// `POST /access/acls/new`
pub async fn acl_create(State(s): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let f = parse_body(&body);
    match s.source.acl_create(&fields(&f)).await {
        AclCreateResult::Created => Redirect::to(&pages::access_url(&s.base)).into_response(),
        AclCreateResult::Invalid(v) => {
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                pages::acl_create_dialog(&v, &s.base),
            )
            .await
        }
        AclCreateResult::Refused(v) => {
            dialog(
                &s,
                StatusCode::OK,
                &headers,
                pages::acl_create_dialog(&v, &s.base),
            )
            .await
        }
    }
}

/// The ACL a form names, or none if it names nothing an ACL could be.
fn target(f: &AclFields<'_>) -> Option<AclEntry> {
    let e = entry_of(&input_of(f));
    missing(&e).is_none().then_some(e)
}

async fn no_acl(s: &Shared) -> Response {
    let main = maud::html! {
        (crate::ui::layout::page_header("No such ACL", maud::html! {
            p class="sub" { "That address does not name an ACL: it needs the resource, the principal and the operation." }
        }, maud::html! {}))
        div class="row" { a class="btn pri" href=(pages::access_url(&s.base)) { "Back to Access" } }
    };
    shell(s, StatusCode::NOT_FOUND, main).await
}

/// `GET /access/acls/delete?type=&name=&pattern=&principal=&host=&op=&perm=`
pub async fn acl_delete_form(
    State(s): State<Shared>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = parse(q.as_deref().unwrap_or(""));
    let Some(entry) = target(&fields(&f)) else {
        return no_acl(&s).await;
    };
    let v = s.source.acl_delete_form(entry).await;
    dialog(
        &s,
        StatusCode::OK,
        &headers,
        pages::acl_delete_dialog(&v, &s.base),
    )
    .await
}

/// `POST /access/acls/delete`
pub async fn acl_delete(State(s): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let f = parse_body(&body);
    let Some(entry) = target(&fields(&f)) else {
        return no_acl(&s).await;
    };
    match s.source.acl_delete(entry).await {
        AclDeleteResult::Deleted => Redirect::to(&pages::access_url(&s.base)).into_response(),
        AclDeleteResult::Gone(v) | AclDeleteResult::Refused(v) => {
            dialog(
                &s,
                StatusCode::OK,
                &headers,
                pages::acl_delete_dialog(&v, &s.base),
            )
            .await
        }
    }
}
