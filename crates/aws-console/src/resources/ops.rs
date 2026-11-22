//! The operations of a kind that no resource is bound to ([`super::Kind::ops`]): a create with a form of its own (`CreateQueue`
//! with its attributes), a bulk delete. `POST …/<service>/<kind>/op/<Operation>`, one call, made as the caller `console`.
//!
//! The binding is a resource operation's ([`super::deep::bound`]): the members of `bind` are set over the input, filled with
//! what no resource gives (`{account}`, `{region}`), so a whitelisted operation can be made to act only inside the account and
//! region of the console, or with a member fixed. `{id}` and `{name}` have no value here and leave their member out.

use aws_wire::ApiError;
use serde_json::{Map, Value};

use super::bus::Bus;
use super::deep::{bound, not_made};
use super::{Descriptor, Kind, Op};

/// The operation `name` of `kind`'s own operations, when it has one.
#[must_use]
pub fn allowed(kind: &Kind, name: &str) -> Option<&'static Op> {
    kind.ops.iter().find(|o| o.op == name)
}

impl Op {
    /// The same operation, of `service` (its name on the bus).
    #[must_use]
    pub const fn of(mut self, service: &'static str) -> Self {
        self.service = Some(service);
        self
    }

    /// The service it is made on: its own, else the descriptor's.
    #[must_use]
    pub fn target(&self, d: &Descriptor) -> &'static str {
        self.service.unwrap_or(d.target)
    }
}

/// An operation of a kind that binds nothing: the page's input goes as it is.
#[must_use]
pub const fn free(op: &'static str) -> Op {
    super::op(op, "{}")
}

/// Makes operation `operation` of `kind`'s own operations with the page's input; the service's answer.
///
/// # Errors
/// 404 `NOT_IN_PAGE` for an operation the kind does not list; the service's refusal, IAM's included.
pub async fn call(
    bus: &Bus,
    (d, kind): (&Descriptor, &Kind),
    operation: &str,
    input: Map<String, Value>,
) -> Result<Value, ApiError> {
    let op = allowed(kind, operation).ok_or_else(|| {
        let mut e = not_made(kind, operation);
        e.code = "NOT_IN_PAGE".into();
        e
    })?;
    let body = Value::Object(bound(bus, op, ("", ""), input)?);
    match bus.call(op.target(d), op.op, &body).await {
        Err(e) => match calm(kind, operation, &e) {
            Some(answer) => Ok(answer),
            None => Err(e),
        },
        answered => answered,
    }
}

/// The answer a refusal stands for, when the kind says that refusal is only "there is nothing" ([`Kind::calm`]).
fn calm(kind: &Kind, operation: &str, e: &ApiError) -> Option<Value> {
    let c = kind
        .calm
        .iter()
        .find(|c| c.op == operation && e.code == c.code && e.message.contains(c.says))?;
    serde_json::from_str(c.answer).ok()
}
