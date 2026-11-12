//! The Access page and its two dialogs, Create ACL and Delete ACL, each with its POST. A dialog is a route that answers its
//! body alone to a script (`X-Fragment`) and the whole page with the dialog in it to a browser without one. A GET never changes
//! anything; every change is a POST with the same-origin proof, and a success goes back to the page (303).

use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;

use super::html;
use crate::form::{get, parse, parse_body, text};
use crate::gather::access_dialogs::{AclFields, entry_of, input_of, missing};
use crate::model::{AclCreateResult, AclCreateView, AclDeleteResult, AclDeleteView, AclEntry};
use crate::pages;
use crate::state::Shared;
use crate::ui::client::HEADER_FRAGMENT;
use crate::ui::shell::Rail;

async fn shell(s: &Shared, status: StatusCode, main: Markup) -> Response {
    let doc = s
        .document(
            Rail::Access,
            "Access",
            &[("Home", Some("/")), ("Access", None)],
            &[],
            main,
        )
        .await;
    (status, html(&doc)).into_response()
}

/// A dialog: its body alone for a script; else the Access page with the overlay open on it.
async fn dialog(
    s: &Shared,
    status: StatusCode,
    h: &HeaderMap,
    overlay: impl Fn(bool) -> Markup,
) -> Response {
    if h.contains_key(HEADER_FRAGMENT) {
        return (status, html(&overlay(false))).into_response();
    }
    let parent = pages::access_page(&s.source.access(None).await, &s.base);
    shell(s, status, pages::access_with_overlay(parent, overlay(true))).await
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

fn create_overlay<'a>(s: &'a Shared, v: &'a AclCreateView) -> impl Fn(bool) -> Markup + 'a {
    move |inline| pages::acl_create_overlay(v, &s.base, inline)
}

fn delete_overlay<'a>(s: &'a Shared, v: &'a AclDeleteView) -> impl Fn(bool) -> Markup + 'a {
    move |inline| pages::acl_delete_overlay(v, &s.base, inline)
}

/// `GET /access`
pub async fn access(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    // `?state=secured` is read by the fixtures only; a live broker decides by its data.
    let f = parse(q.as_deref().unwrap_or(""));
    let view = s.source.access(get(&f, "state")).await;
    shell(&s, StatusCode::OK, pages::access_page(&view, &s.base)).await
}

/// `GET /access/acls/new`
pub async fn acl_create_form(
    State(s): State<Shared>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = parse(q.as_deref().unwrap_or(""));
    let v = s.source.acl_create_form(&fields(&f)).await;
    dialog(&s, StatusCode::OK, &headers, create_overlay(&s, &v)).await
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
                create_overlay(&s, &v),
            )
            .await
        }
        AclCreateResult::Refused(v) => {
            dialog(&s, StatusCode::OK, &headers, create_overlay(&s, &v)).await
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
    dialog(&s, StatusCode::OK, &headers, delete_overlay(&s, &v)).await
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
            dialog(&s, StatusCode::OK, &headers, delete_overlay(&s, &v)).await
        }
    }
}
