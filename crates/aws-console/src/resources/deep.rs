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

/// How to tell that a handle the page names is one of this resource's own parts when nothing in the handle says so (an SQS
/// move task's handle): the operation `list`, bound by `bind` as an [`Op`]'s is, lists them; the handle in `member` must be
/// the `at` of one of the `items`.
#[derive(Debug)]
pub struct Owned {
    pub member: &'static str,
    pub list: &'static str,
    pub bind: &'static str,
    pub items: &'static str,
    pub at: &'static str,
}

/// An operation of a [`super::Deep`] page on a part of the resource that [`Owned`] tells is this resource's.
#[must_use]
pub const fn op_owned(op: &'static str, bind: &'static str, owned: &'static Owned) -> Op {
    Op {
        op,
        bind,
        within: None,
        owned: Some(owned),
        service: None,
    }
}

/// 400 `NOT_THIS_RESOURCE` unless the part `input` names is among those `owned.list` lists for this resource.
async fn check_owned(
    bus: &Bus,
    (target, op, owned): (&str, &Op, &Owned),
    (id, name): (&str, &str),
    input: &Map<String, Value>,
) -> Result<(), ApiError> {
    let named = input
        .get(owned.member)
        .and_then(Value::as_str)
        .unwrap_or("");
    let list = Op {
        op: owned.list,
        bind: owned.bind,
        within: None,
        owned: None,
        service: None,
    };
    let ask = bound(bus, &list, (id, name), Map::new())?;
    let answer = bus.call(target, owned.list, &Value::Object(ask)).await?;
    let ours = path::get(&answer, owned.items)
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .any(|item| !named.is_empty() && path::text(item, owned.at).as_deref() == Some(named));
    if ours {
        Ok(())
    } else {
        Err(ApiError::new(
            400,
            "NOT_THIS_RESOURCE",
            format!(
                "{} acts here only on a {} that {} lists for this {name}.",
                op.op, owned.member, owned.list
            ),
        ))
    }
}

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

/// Whether `named` is a part of what `prefix` names. A prefix that ends in a separator (`:`, `/`) is begun by every longer value;
/// one that does not (a state machine's ARN) is the value itself or a part of it after a `:` (its version `…:2`, its alias `…:PROD`),
/// never another name that happens to begin the same (`order` and `order-fulfilment`).
fn within(named: &str, prefix: &str) -> bool {
    if prefix.is_empty() || !named.starts_with(prefix) {
        return false;
    }
    if prefix.ends_with([':', '/']) {
        named.len() > prefix.len()
    } else {
        named.len() == prefix.len()
            || named
                .get(prefix.len()..)
                .is_some_and(|rest| rest.starts_with(':'))
    }
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
        if !within(named, &prefix) {
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

/// What `op`'s binding fills `{id}` with: the kind's id of `name` ([`run::id_of`], which may be a call), or, when nothing `op`
/// binds names `{id}` (a log group's operations take its name), the name itself and no call.
///
/// # Errors
/// The kind has no resource named `name`, or the call that finds it was refused.
pub async fn id_for(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    op: &Op,
    name: &str,
) -> Result<String, ApiError> {
    let uses_id = |t: &str| t.contains("{id}");
    let needed = uses_id(op.bind)
        || op.within.is_some_and(|(_, prefix)| uses_id(prefix))
        || op.owned.is_some_and(|o| uses_id(o.bind));
    if needed {
        Ok(run::id_of(bus, d, kind, name).await?.0)
    } else {
        Ok(name.to_owned())
    }
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
    let id = id_for(bus, (d, kind), op, name).await?;
    if let Some(owned) = op.owned {
        check_owned(bus, (op.target(d), op, owned), (&id, name), &input).await?;
    }
    let body = Value::Object(bound(bus, op, (&id, name), input)?);
    if (d.target, op.op) == ("lambda", "Invoke") {
        let done = invoke::invoke(bus, &body).await?;
        return serde_json::to_value(done).map_err(|e| ApiError::internal(e.to_string()));
    }
    bus.call(op.target(d), op.op, &body).await
}
