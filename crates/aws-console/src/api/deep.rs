//! The deep pages' API ([`crate::deep`]): each answer made by one call through the peer bus as the caller `console`, when it is
//! asked.
//!
//! - `POST /api/v1/deep/<service>/<operation>`: the operation (one of [`crate::deep::OPS`]) with the JSON body as its request;
//!   the service's answer as its model has it. A POST for reads too: the request is a JSON document, and every POST is behind
//!   the same-origin proof. `lambda/Invoke` answers [`crate::model::Invoked`] (the payload as text).
//! - `GET /api/v1/deep/logs/tail?group=<name>[&stream=<name>]…[&pattern=<text>]`: a Live Tail session as server-sent events
//!   ([`crate::deep::tail`]); one of the console's event streams ([`crate::limits::Limits::max_streams`]).
//!
//! A refusal by the service is answered with its own status, code and words.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use super::calls::KEEP_ALIVE;
use super::resources::decode;
use super::{busy, error, json};
use crate::deep::{self, invoke, tail};
use crate::resources::view;
use crate::state::Shared;

fn refused(e: &aws_wire::ApiError) -> Response {
    let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_GATEWAY);
    error(status, &e.code, &e.message)
}

fn not_served(service: &str) -> Response {
    error(
        StatusCode::CONFLICT,
        "NOT_SERVED",
        &format!(
            "This doze-aws does not serve {service} (--services), so the console leaves it asleep."
        ),
    )
}

fn no_bus() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "NO_BUS",
        "This console was started without the services' bus, so it cannot read resources.",
    )
}

/// `POST /api/v1/deep/<service>/<operation>`.
pub async fn call(
    State(s): State<Shared>,
    Path((service, operation)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let Some(op) = deep::op(&service, &operation) else {
        return error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!("The console's pages do not ask {service} for {operation}."),
        );
    };
    if !view::served(&s.facts, op.service) {
        return not_served(op.service);
    }
    let Some(bus) = s.bus() else {
        return no_bus();
    };
    let asked: Value = match serde_json::from_slice(&body) {
        Ok(v @ Value::Object(_)) => v,
        _ => {
            return error(
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                &format!("{operation}'s request is a JSON object."),
            );
        }
    };
    if op.service == "lambda" && op.operation == "Invoke" {
        return match invoke::invoke(&bus, &asked).await {
            Ok(done) => json(StatusCode::OK, &done),
            Err(e) => refused(&e),
        };
    }
    match deep::call(&bus, op, &asked).await {
        Ok(answer) => json(StatusCode::OK, &answer),
        Err(e) => refused(&e),
    }
}

/// What a tail's address asks for: `group` once, `stream` any number of times, `pattern` at most once.
#[must_use]
pub fn ask_of(query: Option<&str>) -> tail::Ask {
    let mut ask = tail::Ask::default();
    for (key, value) in query
        .unwrap_or("")
        .split('&')
        .filter_map(|p| p.split_once('='))
    {
        let value = decode(value);
        match key {
            "group" => ask.group = value,
            "stream" if !value.is_empty() => ask.streams.push(value),
            "pattern" => ask.pattern = value,
            _ => {}
        }
    }
    ask
}

/// `GET /api/v1/deep/logs/tail`.
pub async fn live_tail(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let ask = ask_of(query.as_deref());
    if ask.group.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "A Live Tail names its log group (?group=).",
        );
    }
    if !view::served(&s.facts, "logs") {
        return not_served("logs");
    }
    let Some(bus) = s.bus() else {
        return no_bus();
    };
    let Ok(slot) = Arc::clone(&s.streams).try_acquire_owned() else {
        return busy(
            "The console has as many event streams open as it allows. Close a tail in another tab and start this one again.",
        );
    };
    let body = tail::request(&ask, (bus.account(), bus.region()));
    match bus.stream("logs", "StartLiveTail", &body).await {
        Ok(download) => Sse::new(tail::events(download, s.stop.clone(), slot))
            .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive"))
            .into_response(),
        Err(e) => refused(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::ask_of;

    #[test]
    fn a_tail_address_is_read_with_its_escapes() {
        let ask = ask_of(Some(
            "group=%2Fapp%2Fapi&stream=web-1&stream=web%202&pattern=%3FERROR%20%3FWARN&x=1",
        ));
        assert_eq!(ask.group, "/app/api");
        assert_eq!(ask.streams, ["web-1", "web 2"]);
        assert_eq!(ask.pattern, "?ERROR ?WARN");
        assert!(ask_of(None).group.is_empty());
    }
}
