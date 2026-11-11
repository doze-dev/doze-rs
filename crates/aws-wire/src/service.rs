//! What the gateway and a service say to each other. No HTTP in it: a service is written against these types and is tested
//! without a socket.

use std::future::Future;
use std::pin::Pin;

use aws_schema::arena::Arena;
use aws_schema::json::Violation;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;

use crate::{ApiError, RequestId};

/// Who is calling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    /// The access key id on the request.
    pub access_key_id: String,
    /// The account the request is for.
    pub account_id: String,
    /// The region it was signed for.
    pub region: String,
}

/// One decoded request, owned, so it can cross to the task that owns the service.
#[derive(Debug)]
pub struct Call {
    /// The operation: an index into the service's tables.
    pub op: u32,
    /// The request id (the answer carries the same).
    pub id: RequestId,
    /// Who is calling.
    pub caller: Caller,
    /// The request body, which the arena's strings point into.
    pub body: String,
    /// The decoded tree.
    pub arena: Arena,
    /// The root node of the decoded input.
    pub root: u32,
}

/// An answer: a tree to encode as the operation's output shape.
#[derive(Debug)]
pub struct Output {
    /// The tree.
    pub arena: Arena,
    /// Its root node.
    pub root: u32,
}

/// What a call produces.
pub type Reply = Result<Output, ApiError>;

/// A boxed future, so a service can sit behind a trait object (one allocation per call).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A service the gateway can route to.
pub trait Service: Send + Sync + 'static {
    /// What its model says about it.
    fn meta(&self) -> &'static ServiceMeta;

    /// Its model's tables.
    fn tables(&self) -> &'static Tables;

    /// Handles one call. An error names a modelled shape ([`ApiError::modeled`]) or states itself.
    fn call(&self, call: Call) -> BoxFuture<'_, Reply>;

    /// How a request that fails its model's checks is answered. The default is the Coral front end's
    /// `ValidationException`; a service whose real answer differs (SQS says `InvalidParameterValue` or `MissingParameter`)
    /// overrides it.
    fn invalid(&self, violation: &Violation) -> ApiError {
        default_invalid(self.tables(), violation)
    }
}

/// The Coral front end's wording of a failed model check.
#[must_use]
pub fn default_invalid(tables: &Tables, violation: &Violation) -> ApiError {
    let member = |ix: u32| tables.member(ix).map_or("", |m| m.name);
    let message = match violation {
        Violation::Syntax { .. } | Violation::TooDeep => {
            "Unable to parse the request body.".to_owned()
        }
        Violation::Type {
            member: m,
            expected,
        } => format!("The value of '{}' must be a {expected}.", member(*m)),
        Violation::Missing { member: m, .. } => {
            format!("The request must contain the parameter {}.", member(*m))
        }
        Violation::Length { member: m } => format!(
            "Value at '{}' failed to satisfy its length constraint.",
            member(*m)
        ),
        Violation::Range { member: m } => format!(
            "Value at '{}' failed to satisfy its range constraint.",
            member(*m)
        ),
        Violation::Pattern { member: m } => format!(
            "Value at '{}' failed to satisfy its pattern constraint.",
            member(*m)
        ),
        Violation::Enum { member: m } => format!(
            "Value at '{}' is not one of the allowed values.",
            member(*m)
        ),
        Violation::Union { .. } => "Exactly one member of a union may be set.".to_owned(),
        _ => "The request is not valid.".to_owned(),
    };
    ApiError::new(400, "ValidationException", message)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_model_sqs::tables::TABLES;
    use aws_schema::json::decode;
    use aws_schema::patterns::Patterns;

    use super::*;

    #[test]
    fn a_missing_member_is_named() {
        let shape = TABLES
            .op(TABLES.find_op("SendMessage").unwrap())
            .unwrap()
            .input
            .unwrap();
        let v = decode(
            &TABLES,
            &Patterns::new(&TABLES),
            shape,
            r#"{"MessageBody":"x"}"#,
            &mut Arena::new(),
        )
        .unwrap_err();
        let e = default_invalid(&TABLES, &v);
        assert_eq!((e.status, e.code.as_ref()), (400, "ValidationException"));
        assert_eq!(
            e.message,
            "The request must contain the parameter QueueUrl."
        );
    }
}
