//! The router: the JSON API, the health check, the bundle's files and `index.html` for every other GET, with the layers in front
//! of all of it: security headers, then the host guard, then the body limit. The API has its own proof (`api::guard`: the
//! `X-Console` header, and the same-origin proof on a write).

use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::files::health;
use crate::guard::{MAX_BODY, body_limit, host_guard};
use crate::headers::security_headers;
use crate::state::Shared;

/// The whole console: its API, its files, and `index.html` for every other GET, so a deep link works.
pub fn console(state: Shared) -> Router {
    crate::api::routes()
        .route("/healthz", get(health))
        .fallback(dispatch)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(from_fn(body_limit))
        .layer(from_fn_with_state(state.clone(), host_guard))
        .layer(from_fn(security_headers))
        .with_state(state)
}

/// Where a request no route took goes: an API 404, a file of the bundle, or `index.html`.
async fn dispatch(State(s): State<Shared>, req: Request<Body>) -> Response {
    let path = req.uri().path();
    let under = |p: &str| path == p || path.starts_with(&format!("{p}/"));
    if under("/api") {
        return crate::api::not_found();
    }
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return crate::api::error(
            StatusCode::METHOD_NOT_ALLOWED,
            "METHOD_NOT_ALLOWED",
            "This address only answers GET.",
        );
    }
    let bundle = s.bundle();
    if let Some(res) = bundle.file(path.trim_start_matches('/'), req.headers()) {
        return res;
    }
    // A name under /assets/ that the bundle does not have is not a page: no index.html for it (and none for `..` or an encoded
    // step in it, which is only a name the bundle does not have).
    if under("/assets") {
        return (StatusCode::NOT_FOUND, "no such file\n").into_response();
    }
    bundle.index()
}
