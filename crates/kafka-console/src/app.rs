//! The router: the JSON API, the health check, the bundle's files and `index.html` for every other GET, with the layers in front
//! of all of it: security headers, then the host guard, then the door to other machines (`door`, SEC-250), then the body limit. The API has its own proof (`api::guard`: the
//! `X-Console` header, and the same-origin proof on a write).

use axum::Router;
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::extract::{Request, State};
use axum::http::{Method as HttpMethod, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state, map_request};
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
    let routes = crate::api::routes(&web)
        .route(&web.url("/healthz"), get(health))
        .fallback(dispatch)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(from_fn(body_limit))
        .layer(from_fn_with_state(state.clone(), crate::door::door_guard))
        .layer(from_fn_with_state(state.clone(), host_guard))
        .layer(from_fn(security_headers))
        .with_state(state);
    // Before the routes are matched, so that `!.` reaches them as the name `.`.
    Router::new()
        .fallback_service(routes)
        .layer(map_request(dot_names))
}

/// A name that is one or two dots, as the page writes it in a path: `!.` and `!..`. A browser (any WHATWG URL parser) takes `.`,
/// `..`, `%2E` and `%2E%2E` as directory steps and removes them before the request is sent, so a group named `.` would turn
/// `/api/v1/groups/./offsets/delete` into `/api/v1/groups/offsets/delete`. `!` is a sub-delimiter (RFC 3986 2.2), which the
/// page's `segment` and the server's `base::segment` always percent-encode, so a raw `!.` comes from no other name.
/// Returns the path with each such segment as `%2E` or `%2E%2E`, which the routes decode to the name; `None` if there is none.
#[must_use]
pub fn undot(path: &str) -> Option<String> {
    let steps = |s: &str| s == "!." || s == "!..";
    if !path.split('/').any(steps) {
        return None;
    }
    let out: Vec<&str> = path
        .split('/')
        .map(|s| match s {
            "!." => "%2E",
            "!.." => "%2E%2E",
            s => s,
        })
        .collect();
    Some(out.join("/"))
}

async fn dot_names(mut req: Request<Body>) -> Request<Body> {
    let Some(path) = undot(req.uri().path()) else {
        return req;
    };
    let with_query = match req.uri().query() {
        Some(q) => format!("{path}?{q}"),
        None => path,
    };
    let mut parts = req.uri().clone().into_parts();
    if let Ok(pq) = with_query.parse() {
        parts.path_and_query = Some(pq);
        if let Ok(uri) = axum::http::Uri::from_parts(parts) {
            *req.uri_mut() = uri;
        }
    }
    req
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

#[cfg(test)]
mod tests {
    use super::undot;

    #[test]
    fn a_dot_name_written_with_its_mark_becomes_an_encoded_segment_and_nothing_else_changes() {
        assert_eq!(
            undot("/api/v1/groups/!./offsets/delete").as_deref(),
            Some("/api/v1/groups/%2E/offsets/delete")
        );
        assert_eq!(
            undot("/api/v1/groups/!..").as_deref(),
            Some("/api/v1/groups/%2E%2E")
        );
        assert_eq!(
            undot("/k/api/v1/topics/!./transactions/!..").as_deref(),
            Some("/k/api/v1/topics/%2E/transactions/%2E%2E")
        );
        for plain in [
            "/api/v1/groups/a.b",
            "/api/v1/groups/%21.",
            "/api/v1/groups/!...",
            "/api/v1/groups/x!.",
            "/api/v1/groups/%2E",
            "/",
        ] {
            assert_eq!(undot(plain), None, "{plain}");
        }
    }
}
