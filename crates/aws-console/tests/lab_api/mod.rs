//! What the Lab API's tests share: a console state with a Lab table (or none), and a rule's body.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::Arc;

use aws_console::app::console;
use aws_console::lab::LabTable;
use aws_console::state::{Shared, State};
use aws_console::{LabFacts, Limits, Recorder};
use axum::Router;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::support::{self, HOST, get, json as answer};

/// A write as the console's own page sends it.
pub(crate) fn write(
    method: &str,
    path: &str,
    body: &Value,
) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header(axum::http::header::HOST, HOST)
        .header("x-console", "1")
        .header(axum::http::header::ORIGIN, format!("http://{HOST}"))
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

pub(crate) struct Lab {
    pub(crate) app: Router,
    pub(crate) state: Shared,
}

pub(crate) fn lab(on: bool) -> Lab {
    let mut facts = support::facts();
    facts.lab = if on {
        LabFacts {
            table: Some(LabTable::new()),
            off_why: None,
            open: false,
        }
    } else {
        LabFacts {
            table: None,
            off_why: Some("The Lab is off: doze-aws was started with --lab=false.".to_owned()),
            open: false,
        }
    };
    let state = State::new(
        facts,
        Arc::new(Recorder::new()),
        HOST.parse().unwrap(),
        Vec::new(),
        CancellationToken::new(),
        &Limits::default(),
    );
    Lab {
        app: console(state.clone()),
        state,
    }
}

impl Lab {
    pub(crate) async fn get(&self, path: &str) -> (u16, Value) {
        answer(&self.app, get(path)).await
    }

    pub(crate) async fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        answer(&self.app, write("POST", path, body)).await
    }
}

pub(crate) fn body(effect: Value) -> Value {
    json!({
        "name": "",
        "matcher": {
            "services": ["sqs"],
            "operations": ["SendMessage"],
            "regions": [],
            "callers": [],
            "resources": ["orders"],
        },
        "effect": effect,
        "share": { "kind": "nth", "n": 4 },
        "ends_in_secs": 600,
    })
}

pub(crate) fn throttle() -> Value {
    body(json!({ "kind": "fail", "error": "throttle" }))
}
