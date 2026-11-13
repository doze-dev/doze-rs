//! The router: the JSON API, the health check, the bundle's files and `index.html` for every other GET, with the layers in front
//! of all of it: security headers, then the host guard, then the body limit. The API has its own proof (`api::guard`: the
//! `X-Console` header, and the same-origin proof on a write).

use axum::Router;
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::extract::{Request, State};
use axum::http::{Method as HttpMethod, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::files::health;
use crate::guard::{MAX_BODY, body_limit, host_guard};
use crate::headers::security_headers;
use crate::state::Shared;

/// The whole console at `state.web` (`--console-prefix`): its API, its files, and `index.html` for every other GET, so a deep
/// link works.
pub fn console(state: Shared) -> Router {
    let web = state.web.clone();
    crate::api::routes(&web)
        .route(&web.url("/healthz"), get(health))
        .fallback(dispatch)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(from_fn(body_limit))
        .layer(from_fn_with_state(state.clone(), host_guard))
        .layer(from_fn(security_headers))
        .with_state(state)
}

/// Where a request no API route took goes: an API 404, a file of the bundle, or `index.html`. Outside the prefix there is
/// nothing.
async fn dispatch(State(s): State<Shared>, req: Request<Body>) -> Response {
    let path = req.uri().path().to_owned();
    let Some(rest) = s.web.strip(&path) else {
        return (StatusCode::NOT_FOUND, "not found\n").into_response();
    };
    let under = |p: &str| rest == p || rest.starts_with(&format!("{p}/"));
    if under("/api") {
        return crate::api::not_found();
    }
    if req.method() != HttpMethod::GET && req.method() != HttpMethod::HEAD {
        return crate::api::error(
            StatusCode::METHOD_NOT_ALLOWED,
            "METHOD_NOT_ALLOWED",
            "This address only answers GET.",
        );
    }
    let file = rest.trim_start_matches('/');
    if let Some(res) = s.bundle.file(file, req.headers()) {
        return res;
    }
    if under("/assets") {
        return (StatusCode::NOT_FOUND, "no such file\n").into_response();
    }
    s.bundle.index()
}
