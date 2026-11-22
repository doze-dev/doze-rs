//! `POST /api/v1/preview`: the call a form would make, written out before it is made.
//!
//! The answer is worked out from the model's tables (the same ones the Lab reads), never by calling the service: `json` is the
//! request as the wire shows it (a JSON protocol's body; a query protocol's form members as an object, a number or a flag keeping
//! its type; a REST protocol's body members, the rest being in the address), `cli` and `boto3` are the call as the AWS CLI and boto3 write it, and `missing`
//! names the required members `input` lacks. An operation doze-aws has not built, or a service it does not have, is 404
//! `NOT_BUILT`.

use aws_schema::meta::Protocol;
use aws_schema::tables::{
    F_REQUIRED, K_BOOL, K_FLOAT, K_INT, K_LIST, K_MAP, K_STRUCT, K_UNION, MemberRef, Tables,
};
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Map, Value};

use super::render::{self, Target};
use super::{error, json};
use crate::model::{Preview, PreviewRequest};
use crate::state::Shared;

/// The members of the structure `shape`, in the model's order.
fn members(tables: &Tables, shape: Option<u32>) -> Vec<MemberRef<'_>> {
    let Some(sh) = shape.and_then(|s| tables.shape(s)) else {
        return Vec::new();
    };
    (sh.first..sh.first + sh.count)
        .filter_map(|i| tables.member(i))
        .collect()
}

/// A scalar as a form value.
fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The form members of a structure's `value` under `prefix` (`Attributes.entry.1.key`), as the awsQuery decoder reads them
/// ([`aws_wire::query::to_json`] is the inverse).
fn flatten(t: &Tables, shape: u32, value: &Value, prefix: &str, out: &mut Map<String, Value>) {
    for m in members(t, Some(shape)) {
        if let Some(v) = value.get(m.wire).filter(|v| !v.is_null()) {
            put(
                t,
                &m,
                v,
                &format!("{prefix}{}", m.xml.unwrap_or(m.name)),
                out,
            );
        }
    }
}

fn put(t: &Tables, m: &MemberRef<'_>, v: &Value, key: &str, out: &mut Map<String, Value>) {
    match (m.kind, v) {
        (K_STRUCT | K_UNION, Value::Object(_)) => flatten(t, m.target, v, &format!("{key}."), out),
        (K_LIST, Value::Array(items)) => {
            let Some(element) = t.member(m.target) else {
                return;
            };
            let base = if m.flags & 2 != 0 {
                key.to_owned()
            } else {
                format!("{key}.{}", element.xml.unwrap_or("member"))
            };
            for (n, item) in items.iter().enumerate() {
                put(t, &element, item, &format!("{base}.{}", n + 1), out);
            }
        }
        (K_MAP, Value::Object(pairs)) => {
            let (Some(k), Some(val)) = (t.member(m.target), t.member(m.target + 1)) else {
                return;
            };
            let base = if m.flags & 2 != 0 {
                key.to_owned()
            } else {
                format!("{key}.entry")
            };
            for (n, (name, item)) in pairs.iter().enumerate() {
                let at = format!("{base}.{}", n + 1);
                out.insert(
                    format!("{at}.{}", k.xml.unwrap_or("key")),
                    Value::String(name.clone()),
                );
                put(
                    t,
                    &val,
                    item,
                    &format!("{at}.{}", val.xml.unwrap_or("value")),
                    out,
                );
            }
        }
        // A number or a flag keeps its JSON type: the panel shows what a person copies into an SDK call, where `DurationSeconds` is
        // a number (the CLI and boto3 tabs are typed too), and a form value is text only on the wire.
        (K_INT | K_FLOAT, Value::Number(_)) | (K_BOOL, Value::Bool(_)) => {
            out.insert(key.to_owned(), v.clone());
        }
        _ => {
            out.insert(key.to_owned(), Value::String(scalar(v)));
        }
    }
}

/// The request as the wire shows it, by `protocol`.
fn wire(
    protocol: Protocol,
    (t, op, version): (&Tables, &aws_schema::tables::OpRef<'_>, &str),
    members: &[MemberRef<'_>],
    input: &Value,
) -> Value {
    match protocol {
        Protocol::AwsQuery => {
            let mut form = Map::new();
            form.insert("Action".to_owned(), Value::String(op.name.to_owned()));
            form.insert("Version".to_owned(), Value::String(version.to_owned()));
            if let Some(shape) = op.input {
                flatten(t, shape, input, "", &mut form);
            }
            Value::Object(form)
        }
        Protocol::RestJson1 | Protocol::RestXml => {
            // What is not in the address (label, query, header) is the body: a payload member alone, else the body members.
            let payload = members.iter().find(|m| m.bind == 6);
            if let Some(m) = payload {
                return input.get(m.wire).cloned().unwrap_or(Value::Null);
            }
            Value::Object(
                members
                    .iter()
                    .filter(|m| m.bind == 0)
                    .filter_map(|m| Some((m.wire.to_owned(), input.get(m.wire)?.clone())))
                    .collect(),
            )
        }
        _ => input.clone(),
    }
}

/// The API of `service`, by its name on the bus, else by its name in `--services`.
fn resolve(s: &Shared, service: &str) -> Option<std::sync::Arc<dyn aws_wire::service::Service>> {
    let all = s.facts.peers.as_ref()?.targets();
    all.iter()
        .find(|(name, _)| *name == service)
        .or_else(|| {
            all.iter()
                .find(|(_, api)| s.facts.service_named(api.meta().signing_name) == service)
        })
        .map(|(_, api)| api.clone())
}

/// The preview of `operation` of `api` for `input`; `None` for an operation it has not built.
fn build(
    s: &Shared,
    api: &dyn aws_wire::service::Service,
    operation: &str,
    input: &Value,
) -> Option<Preview> {
    let (tables, meta) = (api.tables(), api.meta());
    let op = tables
        .find_op(operation)
        .filter(|i| api.serves(*i))
        .and_then(|i| tables.op(i))?;
    let protocol = meta
        .protocols
        .iter()
        .copied()
        .find(|p| !matches!(p, Protocol::Rpcv2Cbor))?;
    let all = members(tables, op.input);
    let missing = all
        .iter()
        .filter(|m| m.flags & F_REQUIRED != 0 && input.get(m.wire).is_none_or(Value::is_null))
        .map(|m| m.wire.to_owned())
        .collect();
    let args = render::known(&all, input);
    let at = Target {
        endpoint: s.facts.endpoint.trim_end_matches('/'),
        region: &s.facts.region,
    };
    Some(Preview {
        json: wire(protocol, (tables, &op, meta.version), &all, input),
        cli: render::cli(meta.sdk_id, op.name, at, &args),
        boto3: render::boto3(meta.sdk_id, op.name, at, &args),
        missing,
    })
}

/// `POST /api/v1/preview`.
pub async fn preview(State(s): State<Shared>, body: Bytes) -> Response {
    let Ok(req) = serde_json::from_slice::<PreviewRequest>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "A preview is {\"service\": \"sqs\", \"operation\": \"CreateQueue\", \"input\": {…}}.",
        );
    };
    let input = if req.input.is_object() {
        req.input.clone()
    } else {
        Value::Object(Map::new())
    };
    match resolve(&s, &req.service).and_then(|api| build(&s, api.as_ref(), &req.operation, &input))
    {
        Some(p) => json(StatusCode::OK, &p),
        None => error(
            StatusCode::NOT_FOUND,
            "NOT_BUILT",
            &format!(
                "{} {} is not an operation doze-aws has built.",
                req.service, req.operation
            ),
        ),
    }
}
