//! The operations of a resource's own pages ([`super::Deep`]): one call, named by the page and allowed by the descriptor, made
//! as the caller `console` on that resource alone.

use aws_wire::ApiError;
use serde_json::{Map, Value};

use super::bus::Bus;
use super::run::{self, Fill};
use super::{Descriptor, Kind, Op, path};
use crate::model::DeepTab;

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

/// `input` with the members of `op`'s `bind` set over it, filled for the resource `id` named `name`.
///
/// # Errors
/// A descriptor whose `bind` is not a JSON object (a fault of the build).
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
    let Value::Object(set) = path::fill(&template, &fill) else {
        return Err(ApiError::internal("a descriptor's bind is not an object"));
    };
    input.extend(set);
    Ok(input)
}

/// Makes operation `operation` of `kind`'s pages on the resource named `name`, with the page's `input`; the service's answer.
///
/// # Errors
/// 404 `NOT_FOUND` for an operation no page of the kind makes; the service's refusal, IAM's included.
pub async fn call(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    name: &str,
    operation: &str,
    input: Map<String, Value>,
) -> Result<Value, ApiError> {
    let op = allowed(kind, operation).ok_or_else(|| {
        ApiError::new(
            404,
            "NOT_FOUND",
            format!(
                "The console does not make {operation} on a {} from its pages.",
                kind.one
            ),
        )
    })?;
    let (id, _) = run::id_of(bus, d, kind, name).await?;
    let body = bound(bus, op, (&id, name), input)?;
    bus.call(d.target, op.op, &Value::Object(body)).await
}
