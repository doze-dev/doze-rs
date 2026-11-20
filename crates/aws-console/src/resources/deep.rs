//! The operations of a resource's own pages ([`super::Deep`]): one call, named by the page and allowed by the descriptor, made
//! as the caller `console` on that resource alone.
//!
//! Every operation goes through [`call`] with its descriptor's binding. Two are more than a pass-through, and keep the same
//! binding: Lambda's `Invoke` ([`invoke`]: the payload in and out as text, the log tail decoded) answers through [`call`]
//! too; CloudWatch Logs' `StartLiveTail` ([`tail`]: its event stream sent to the browser as server-sent events) has an
//! address of its own (`GET …/<name>/tail`, `api/deep.rs`), whose request is bound by [`bound`] as any other.

pub mod frames;
pub mod invoke;
pub mod tail;

use aws_wire::ApiError;
use serde_json::{Map, Value};

use super::bus::Bus;
use super::run::{self, Fill};
use super::{Descriptor, Kind, Op, path};
use crate::model::DeepTab;

/// The operations whose answer is a stream, each read at an address of its own and never through [`call`].
pub const STREAMED: &[&str] = &["StartLiveTail"];

/// The tabs a detail page shows for `kind`.
#[must_use]
pub fn tabs(kind: &Kind) -> Vec<DeepTab> {
    kind.deep
        .iter()
        .map(|d| DeepTab {
            id: d.id.to_owned(),
            title: d.title.to_owned(),
            operations: d.ops.iter().map(|o| o.op.to_owned()).collect(),
        })
        .collect()
}

/// The operation `name` of `kind`'s pages, when one of them may make it.
#[must_use]
pub fn allowed(kind: &Kind, name: &str) -> Option<&'static Op> {
    kind.deep
        .iter()
        .flat_map(|d| d.ops.iter())
        .find(|o| o.op == name)
}

/// 404: no page of `kind` makes `operation`.
#[must_use]
pub fn not_made(kind: &Kind, operation: &str) -> ApiError {
    ApiError::new(
        404,
        "NOT_FOUND",
        format!(
            "The console does not make {operation} on a {} from its pages.",
            kind.one
        ),
    )
}

/// `input` with the members of `op`'s `bind` set over it (one bound to `null` taken out), filled for the resource `id` named
/// `name`; and, for an operation on a part of it ([`Op::within`]), the part the page names checked to be this resource's.
///
/// # Errors
/// 400 `NOT_THIS_RESOURCE` for a part that is not this resource's (an execution of another machine); a descriptor whose
/// `bind` is not a JSON object (a fault of the build).
pub fn bound(
    bus: &Bus,
    op: &Op,
    (id, name): (&str, &str),
    mut input: Map<String, Value>,
) -> Result<Map<String, Value>, ApiError> {
    let template: Value = serde_json::from_str(op.bind)
        .map_err(|e| ApiError::internal(format!("a descriptor's bind is not JSON: {e}")))?;
    let (out, form) = (Value::Null, Map::new());
    let fill = Fill {
        id,
        name,
        ..Fill::bare(bus, &out, &form)
    };
    if let Some((member, prefix)) = op.within {
        let prefix = path::text(&path::fill(&Value::String(prefix.to_owned()), &fill), "")
            .unwrap_or_default();
        let named = input.get(member).and_then(Value::as_str).unwrap_or("");
        if prefix.is_empty() || named.len() <= prefix.len() || !named.starts_with(&prefix) {
            return Err(ApiError::new(
                400,
                "NOT_THIS_RESOURCE",
                format!(
                    "{} acts here only on a {member} that begins {prefix}.",
                    op.op
                ),
            ));
        }
    }
    let Value::Object(set) = path::fill(&template, &fill) else {
        return Err(ApiError::internal("a descriptor's bind is not an object"));
    };
    for (member, value) in set {
        if value.is_null() {
            input.remove(&member);
        } else {
            input.insert(member, value);
        }
    }
    Ok(input)
}

/// Makes operation `operation` of `kind`'s pages on the resource named `name`, with the page's input; the service's answer
/// (for Lambda's `Invoke`, [`crate::model::Invoked`]).
///
/// # Errors
/// 404 `NOT_FOUND` for an operation no page of the kind makes; 400 for one whose answer is a stream ([`STREAMED`]) or that
/// names a part of another resource; the service's refusal, IAM's included.
pub async fn call(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    name: &str,
    operation: &str,
    input: Map<String, Value>,
) -> Result<Value, ApiError> {
    let op = allowed(kind, operation).ok_or_else(|| not_made(kind, operation))?;
    if STREAMED.contains(&op.op) {
        return Err(ApiError::new(
            400,
            "STREAMED",
            format!("{operation} answers a stream: the page reads it at …/tail."),
        ));
    }
    let (id, _) = run::id_of(bus, d, kind, name).await?;
    let body = Value::Object(bound(bus, op, (&id, name), input)?);
    if (d.target, op.op) == ("lambda", "Invoke") {
        let done = invoke::invoke(bus, &body).await?;
        return serde_json::to_value(done).map_err(|e| ApiError::internal(e.to_string()));
    }
    bus.call(d.target, op.op, &body).await
}
