//! The full pages.

use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::http::Uri;
use axum::response::{IntoResponse, Response};

use super::html;
use crate::pages;
use crate::state::Shared;
use crate::ui::shell::Rail;

/// Home, in the state its data calls for.
pub async fn home(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    // `?state=quiet` or `?state=attention` is read by the fixtures only; a live broker decides by its data.
    let fields = crate::form::parse(query.as_deref().unwrap_or(""));
    let view = s.source.home(crate::form::get(&fields, "state")).await;
    let main = pages::home(&view, &s.base);
    let title = crate::words::home_title(&view.state);
    html(
        &s.document(Rail::Home, title, &[("Home", None)], s.source.watch(), main)
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
            &[("Home", Some("/")), ("Connect an app", None)],
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
    let crumbs = [
        ("Home", Some("/")),
        ("Connect", Some("/connect")),
        ("Advertised listener", None),
    ];
    html(
        &s.document(Rail::Home, "Advertised listener", &crumbs, &[], main)
            .await,
    )
}

/// A page that a later slice builds.
pub async fn planned(s: Shared, title: &'static str) -> Response {
    let main = pages::planned(title, &s.base);
    let rail = match title {
        "Live" => Rail::Live,
        "Access" => Rail::Access,
        "Lab" => Rail::Lab,
        "Settings" => Rail::Settings,
        _ => Rail::Resources,
    };
    html(
        &s.document(
            rail,
            title,
            &[("Home", Some("/")), (title, None)],
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
            &[("Home", Some("/")), ("Not found", None)],
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
            &[("Home", Some("/")), ("Not allowed", None)],
            &[],
            main,
        )
        .await;
    (StatusCode::METHOD_NOT_ALLOWED, html(&page)).into_response()
}
