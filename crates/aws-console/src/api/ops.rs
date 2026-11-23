//! `POST /api/v1/resources/<service>/<kind>/op/<Operation>`: an operation of the kind that no resource is bound to (a create
//! with a form of its own), one of those the kind lists ([`crate::resources::Kind::ops`]), made as the console.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Map, Value};

use super::resources::{find, refused};
use super::{error, json};
use crate::resources::ops;
use crate::state::Shared;

/// The JSON object a request body is (empty for none), or the 400 that says it is not.
pub(super) fn object(body: &Bytes) -> Result<Map<String, Value>, Box<Response>> {
    if body.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => Ok(m),
        _ => Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "An operation's input is a JSON object.",
        ))),
    }
}

/// `POST /api/v1/resources/<service>/<kind>/op/<operation>`.
pub async fn op(
    State(s): State<Shared>,
    Path((service, kind, operation)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (bus, d, k) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let input = match object(&body) {
        Ok(input) => input,
        Err(res) => return *res,
    };
    match ops::call(&bus, (d, k), &operation, input).await {
        Ok(answer) => json(StatusCode::OK, &answer),
        Err(e) => refused(&e),
    }
}
