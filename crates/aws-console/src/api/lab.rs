//! The Lab: any operation a service has built, sent through the bus as the console, its answer shown and its Live entry found.
//!
//! - `GET /api/v1/lab`: every API on the bus with the operations it answers (the model's tables, counted when asked).
//! - `GET /api/v1/lab/<target>/<operation>`: the operation's input members from the model, and a starting input.
//! - `POST /api/v1/lab/<target>/<operation>` (a JSON object): the call, made; its output or error, and its number in Live.
//!
//! The call goes through the service's own model and authorizer, as every console call does: what it may do is what the
//! person running doze-aws may do (see `resources::bus`). Behind the same-origin proof, as every write.

use std::time::Instant;

use aws_schema::tables::{
    F_REQUIRED, K_BLOB, K_BOOL, K_EVENT, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STREAM, K_STRUCT,
    K_TS, K_UNION, Tables,
};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Map, Value};

use super::lab_body::{body_max, cannot, split};
use super::{error, json};
use crate::model::{LabAnswer, LabError, LabIndex, LabMember, LabOperation, LabTarget};
use crate::resources::bus::CONSOLE;
use crate::state::Shared;

fn kind_word(kind: u8) -> &'static str {
    match kind {
        K_BOOL => "boolean",
        K_INT => "integer",
        K_FLOAT => "float",
        K_STR => "string",
        K_BLOB => "blob",
        K_STREAM => "stream",
        K_EVENT => "event stream",
        K_TS => "timestamp",
        K_LIST => "list",
        K_MAP => "map",
        K_STRUCT => "structure",
        K_UNION => "union",
        _ => "value",
    }
}

/// An empty value of a kind, for a starting input.
fn empty(kind: u8) -> Value {
    match kind {
        K_BOOL => Value::Bool(false),
        K_INT | K_FLOAT => Value::from(0),
        K_LIST => Value::Array(Vec::new()),
        K_MAP | K_STRUCT | K_UNION => Value::Object(Map::new()),
        _ => Value::String(String::new()),
    }
}

/// `GET /api/v1/lab`.
pub async fn index(State(s): State<Shared>) -> Response {
    let Some(peers) = s.facts.peers.as_ref() else {
        return json(
            StatusCode::OK,
            &LabIndex {
                targets: Vec::new(),
            },
        );
    };
    let targets = peers
        .targets()
        .into_iter()
        .map(|(target, api)| {
            let tables = api.tables();
            let mut operations = Vec::new();
            let mut i = 0;
            while let Some(op) = tables.op(i) {
                if api.serves(i) {
                    operations.push(op.name.to_owned());
                }
                i += 1;
            }
            operations.sort();
            let meta = api.meta();
            let service = s.facts.service_named(meta.signing_name);
            let found = s.facts.services.iter().find(|v| v.name == service);
            LabTarget {
                target: target.to_owned(),
                sdk_id: meta.sdk_id.to_owned(),
                service: service.to_owned(),
                title: found.map_or_else(|| meta.sdk_id.to_owned(), |v| v.title.to_owned()),
                served: found.is_some_and(|v| v.served),
                operations,
            }
        })
        .collect();
    json(StatusCode::OK, &LabIndex { targets })
}

/// The tables of `target` and the index of its built `operation`, or the answer that says why not.
fn find(
    s: &Shared,
    target: &str,
    operation: &str,
) -> Result<(&'static Tables, u32), Box<Response>> {
    let api = s
        .facts
        .peers
        .as_ref()
        .and_then(|p| p.targets().into_iter().find(|(name, _)| *name == target))
        .map(|(_, api)| api)
        .ok_or_else(|| {
            Box::new(error(
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                &format!("No service is called {target} on the bus."),
            ))
        })?;
    let tables = api.tables();
    match tables.find_op(operation) {
        Some(i) if api.serves(i) => Ok((tables, i)),
        Some(_) => Err(Box::new(error(
            StatusCode::NOT_FOUND,
            "NOT_BUILT",
            &format!("{target} {operation} is in AWS's model and not built in doze-aws."),
        ))),
        None => Err(Box::new(error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!("{target} has no operation {operation}."),
        ))),
    }
}

/// The input members of operation `op`, and a starting input of the required ones.
fn members(tables: &Tables, op: u32) -> (Vec<LabMember>, String) {
    let mut out = Vec::new();
    let mut example = Map::new();
    if let Some(shape) = tables
        .op(op)
        .and_then(|o| o.input)
        .and_then(|i| tables.shape(i))
    {
        for ix in shape.first..shape.first + shape.count {
            let Some(m) = tables.member(ix) else { continue };
            let required = m.flags & F_REQUIRED != 0;
            let values = tables
                .cons(m.cons)
                .filter(|_| m.cons != 0)
                .map(|c| {
                    tables
                        .enum_values(c.enum_set)
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            if required {
                example.insert(m.wire.to_owned(), empty(m.kind));
            }
            out.push(LabMember {
                name: m.wire.to_owned(),
                kind: kind_word(m.kind).to_owned(),
                required,
                values,
            });
        }
    }
    let example =
        serde_json::to_string_pretty(&Value::Object(example)).unwrap_or_else(|_| "{}".to_owned());
    (out, example)
}

/// `GET /api/v1/lab/<target>/<operation>`.
pub async fn operation(
    State(s): State<Shared>,
    Path((target, operation)): Path<(String, String)>,
) -> Response {
    let (tables, op) = match find(&s, &target, &operation) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let (members, example) = members(tables, op);
    let (body_max, cannot) = (body_max(&target, &operation), cannot(&target, &operation));
    json(
        StatusCode::OK,
        &LabOperation {
            target,
            operation,
            members,
            example,
            body_max,
            cannot,
        },
    )
}

/// `POST /api/v1/lab/<target>/<operation>`.
pub async fn send(
    State(s): State<Shared>,
    Path((target, operation)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    if let Err(res) = find(&s, &target, &operation) {
        return *res;
    }
    let Ok(input @ Value::Object(_)) = serde_json::from_slice::<Value>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "The input is a JSON object.",
        );
    };
    let Some(bus) = s.bus() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "NO_BUS",
            "This console was started without the services' bus.",
        );
    };
    // An object's body travels as an upload's does (`lab_body`); every other call is its JSON input.
    let (input, payload) = match split(&target, &operation, input) {
        Ok(split) => split,
        Err(res) => return *res,
    };
    let before = s.recorder.newest_seq();
    let started = Instant::now();
    let answered = match payload {
        Some(bytes) => bus
            .call_rest(&target, &operation, &input, Some(bytes))
            .await
            .map(|reply| reply.json),
        None => bus.call(&target, &operation, &input).await,
    };
    let took_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    // The call's record: the console's first call of this operation after the ones held before it.
    let seq = s
        .recorder
        .after(before, usize::MAX)
        .calls
        .iter()
        .find(|c| &*c.caller == CONSOLE && c.action == operation)
        .map(|c| c.seq);
    let answer = match answered {
        Ok(output) => LabAnswer {
            output: Some(serde_json::to_string_pretty(&output).unwrap_or_default()),
            error: None,
            seq,
            took_us,
        },
        Err(e) => LabAnswer {
            output: None,
            error: Some(LabError {
                status: e.status,
                code: e.code.to_string(),
                message: e.message.clone(),
            }),
            seq,
            took_us,
        },
    };
    json(StatusCode::OK, &answer)
}

#[cfg(test)]
mod tests {
    use super::{empty, kind_word};
    use aws_schema::tables::{K_LIST, K_STR, K_STRUCT};

    #[test]
    fn each_kind_has_a_word_and_an_empty_value() {
        assert_eq!(kind_word(K_STR), "string");
        assert_eq!(empty(K_LIST), serde_json::json!([]));
        assert_eq!(empty(K_STRUCT), serde_json::json!({}));
    }
}
