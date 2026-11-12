//! The full pages.

use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::http::Uri;
use axum::response::{IntoResponse, Response};

use super::html;
use crate::pages;
use crate::state::Shared;
use crate::trail::{Place, borrowed};
use crate::ui::shell::Rail;

/// Home, in the state its data calls for.
pub async fn home(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    // `?state=quiet` or `?state=attention` is read by the fixtures only; a live broker decides by its data.
    let fields = crate::form::parse(query.as_deref().unwrap_or(""));
    let view = s.source.home(crate::form::get(&fields, "state")).await;
    let main = pages::home(&view, &s.base);
    let title = crate::words::home_title(&view.state);
    html(
        &s.document(
            Rail::Home,
            title,
            &borrowed(&Place::Home.steps()),
            crate::regions::HOME,
            main,
        )
        .await,
    )
}

/// Connect.
pub async fn connect(State(s): State<Shared>) -> Response {
    let view = s.source.connect().await;
    let main = pages::connect(&view, &s.base);
    html(
        &s.document(
            Rail::Home,
            "Connect an app",
            &borrowed(&Place::Connect.steps()),
            &[],
            main,
        )
        .await,
    )
}

/// The advertised-listener page.
pub async fn advertised(State(s): State<Shared>) -> Response {
    let view = s.source.probe(false).await;
    let main = pages::advertised(&view, &s.base);
    html(
        &s.document(
            Rail::Home,
            "Advertised listener",
            &borrowed(&Place::Advertised.steps()),
            &[],
            main,
        )
        .await,
    )
}

/// A path that is no page: a 404 in the shell.
pub async fn not_found(State(s): State<Shared>, uri: Uri) -> Response {
    let main = pages::not_found(uri.path(), &s.base);
    let page = s
        .document(
            Rail::Home,
            "Not found",
            &borrowed(&Place::NotFound.steps()),
            &[],
            main,
        )
        .await;
    (StatusCode::NOT_FOUND, html(&page)).into_response()
}

/// A method the path does not take: a 405 in the shell.
pub async fn method_not_allowed(State(s): State<Shared>, uri: Uri) -> Response {
    let main = pages::not_found(uri.path(), &s.base);
    let page = s
        .document(
            Rail::Home,
            "Not allowed",
            &borrowed(&Place::NotAllowed.steps()),
            &[],
            main,
        )
        .await;
    (StatusCode::METHOD_NOT_ALLOWED, html(&page)).into_response()
}
