//! Shared by the live tests of the R1b JSON API: the console (API and all) over a world, and JSON GETs and POSTs.
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
use axum::http::{Request, StatusCode, header};
use kafka_console::base::Base;
use kafka_console::limits::Limits;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::support::{HOST, send};
use crate::support_write::{World, body};

pub fn api(w: &World) -> Router {
    let live = Live::new(w.backend.clone(), w.facts.clone());
    kafka_console::app::console(State::console(
        Source::Live(Box::new(live)),
        &Base::root(),
        vec![],
        CancellationToken::new(),
        &Limits::default(),
    ))
}

pub async fn get(app: &Router, path: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .body(Body::empty())
        .unwrap();
    let (res, text) = send(app, req).await;
    (
        res.status(),
        serde_json::from_str(&text).unwrap_or(Value::Null),
    )
}

pub async fn post(app: &Router, path: &str, fields: &[(&str, &str)]) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header("x-console", "1")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body(fields)))
        .unwrap();
    let (res, text) = send(app, req).await;
    (
        res.status(),
        serde_json::from_str(&text).unwrap_or(Value::Null),
    )
}
