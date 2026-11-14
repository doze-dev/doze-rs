//! Shared by the integration tests: a console over the fixtures, driven with `Router::oneshot`.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, header};
use http_body_util::BodyExt;
use kafka_console::app::console as router;
use kafka_console::base::Base;
use kafka_console::source::Source;
use kafka_console::state::{Shared, State};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

/// A router over the fixtures with the given prefix and allowed hosts.
pub fn app(prefix: &str, allow: &[&str]) -> (Router, Shared) {
    let state = State::new(
        Source::Fixture,
        Base::new(prefix),
        allow.iter().map(|s| (*s).to_owned()).collect(),
        CancellationToken::new(),
    );
    (router(state.clone()), state)
}

pub const HOST: &str = "127.0.0.1:9093";

/// A GET with the right Host.
pub fn get(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap()
}

/// A POST that proves it came from the console's own pages.
pub fn post(path: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

/// Sends `req` and reads the whole answer.
pub async fn send(app: &Router, req: Request<Body>) -> (Response<Body>, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (Response::from_parts(parts, Body::empty()), text)
}

/// The head of the answer, the body unread (for a stream, which has no end).
pub async fn head(app: &Router, req: Request<Body>) -> Response<Body> {
    app.clone().oneshot(req).await.unwrap()
}

/// The local URLs a page links to or loads: every `href` and `src` that starts with a slash.
pub fn local_links(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attr in ["href=\"", "src=\""] {
        let mut rest = html;
        while let Some(at) = rest.find(attr) {
            rest = &rest[at + attr.len()..];
            let end = rest.find('"').unwrap();
            let url = &rest[..end];
            if url.starts_with('/') {
                out.push(url.replace("&amp;", "&"));
            }
        }
    }
    out
}

/// A GET as the React app's fetch client sends it (the API asks for `X-Console: 1`).
pub fn api_get(path: &str) -> Request<Body> {
    let mut req = get(path);
    req.headers_mut()
        .insert("x-console", header::HeaderValue::from_static("1"));
    req
}
