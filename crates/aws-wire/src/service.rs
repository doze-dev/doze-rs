//! What the gateway and a service say to each other. No HTTP in it: a service is written against these types and is tested
//! without a socket.

use std::future::Future;
use std::pin::Pin;

use aws_schema::arena::Arena;
use aws_schema::json::Violation;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::{RangeRef, Tables};

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
    /// The scheme and host the caller used (`http://127.0.0.1:4566`): what a URL in an answer must begin with.
    pub endpoint: String,
    /// The AWS service acting for the caller, when one is (SSM encrypting a `SecureString` through KMS): `None` for a request that
    /// came in over the wire. It is what `kms:ViaService` would say.
    pub service: Option<String>,
    /// The session token the request carried (`X-Amz-Security-Token`), when it carried one.
    pub session_token: Option<String>,
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

impl Call {
    /// A second copy of the call, for a service that must ask again (a long poll re-asks until something arrives). The body
    /// and the tree are copied; a request is small.
    #[must_use]
    pub fn duplicate(&self) -> Self {
        Self {
            op: self.op,
            id: self.id.clone(),
            caller: self.caller.clone(),
            body: self.body.clone(),
            arena: self.arena.clone(),
            root: self.root,
        }
    }
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

/// An unsigned HTTP request to an address a service owns.
#[derive(Debug)]
pub struct WebRequest {
    /// `GET`, `POST`...
    pub method: String,
    /// The `Host` header.
    pub host: String,
    /// The path as sent (still percent-encoded).
    pub path: String,
    /// The query string as sent, without the `?`.
    pub query: String,
    /// The headers, names lower-cased.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: bytes::Bytes,
    /// The scheme and host the client used (`http://127.0.0.1:4566`).
    pub endpoint: String,
}

/// The answer to a [`WebRequest`].
#[derive(Debug)]
pub struct WebReply {
    /// The status.
    pub status: u16,
    /// The headers.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: bytes::Bytes,
}

/// A service with addresses of its own that are not signed API calls (a Lambda function URL).
pub trait Web: Send + Sync {
    /// Whether a request to this host and path is the service's. Cheap and pure: it runs for every request that is not signed.
    fn owns(&self, host: &str, path: &str) -> bool;

    /// Answers the request.
    fn serve(&self, request: WebRequest) -> BoxFuture<'_, WebReply>;
}

/// A service the gateway can route to.
pub trait Service: Send + Sync + 'static {
    /// What its model says about it.
    fn meta(&self) -> &'static ServiceMeta;

    /// Its model's tables.
    fn tables(&self) -> &'static Tables;

    /// Whether the service serves operation `op`. One it does not is refused by name (501 `NotImplemented`, `X-Doze-Not-Built`)
    /// before its body is read, so a service need not carry validation tables for operations it will never run.
    fn serves(&self, _op: u32) -> bool {
        true
    }

    /// Whether every client error is answered 400, whatever status the model gives it (KMS does this).
    fn flat_client_errors(&self) -> bool {
        false
    }

    /// Handles one call. An error names a modelled shape ([`ApiError::modeled`]) or states itself.
    fn call(&self, call: Call) -> BoxFuture<'_, Reply>;

    /// The service's REST side, for a service that speaks a REST protocol (S3): the gateway hands it streaming bodies and takes
    /// streaming answers. `None` for every service that speaks a JSON or Query protocol.
    fn rest(&self) -> Option<&dyn crate::rest::RestService> {
        None
    }

    /// The service's own unsigned web addresses (a Lambda function URL), for a service that has some: the gateway asks it before it
    /// looks for a signature, and hands it the request whole. `None` for every service that has none.
    fn web(&self) -> Option<&dyn Web> {
        None
    }

    /// How a request that fails its model's checks is answered. The default is the Coral front end's
    /// `ValidationException`; a service whose real answer differs (SQS says `InvalidParameterValue` or `MissingParameter`)
    /// overrides it.
    fn invalid(&self, violation: &Violation) -> ApiError {
        default_invalid(self.tables(), violation)
    }

    /// [`Service::invalid`] for a service whose wording quotes the request (DynamoDB prints a rejected batch as Java does). The
    /// default ignores the body.
    fn invalid_in(&self, violation: &Violation, _body: &str) -> ApiError {
        self.invalid(violation)
    }
}

/// The Coral front end's wording of a failed model check. A bad enum value is worded as AWS worded it when the SSM scenario
/// was recorded (2026-11-09): `1 validation error detected: Value 'Sentence' at 'type' failed to satisfy constraint: Member must
/// satisfy enum value set: [SecureString, StringList, String]`. That one case is the evidence for the whole shape, including the
/// enum set listed in the reverse of the model's order; the length, range and pattern sentences follow the same convention
/// from memory and are checked by whichever scenario first reaches them.
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
        Violation::Length { member: m, value }
        | Violation::Range { member: m, value }
        | Violation::Pattern { member: m, value }
        | Violation::Enum { member: m, value } => {
            constraint(tables, violation, *m, value.as_deref())
        }
        Violation::Union { .. } => "Exactly one member of a union may be set.".to_owned(),
        _ => "The request is not valid.".to_owned(),
    };
    ApiError::new(400, "ValidationException", message)
}

/// `Value 'x' at 'member' failed to satisfy constraint: Member must ...`.
#[allow(clippy::cast_precision_loss)]
fn constraint(tables: &Tables, violation: &Violation, ix: u32, value: Option<&str>) -> String {
    let Some(m) = tables.member(ix) else {
        return "The request is not valid.".to_owned();
    };
    let cons = tables.cons(m.cons);
    let must = match (violation, cons) {
        (Violation::Enum { .. }, Some(c)) => {
            let mut values = tables.enum_values(c.enum_set);
            values.reverse();
            format!("satisfy enum value set: [{}]", values.join(", "))
        }
        (Violation::Pattern { .. }, Some(c)) => {
            format!(
                "satisfy regular expression pattern: {}",
                // AWS prints the pattern as the model writes it, without the anchors the tables add.
                c.pattern
                    .unwrap_or_default()
                    .trim_start_matches('^')
                    .trim_end_matches('$')
            )
        }
        (Violation::Length { .. }, Some(c)) => {
            let n = value.map(|v| v.chars().count() as f64);
            bound(
                "length",
                n,
                c.len_min.map(|x| x as f64),
                c.len_max.map(|x| x as f64),
            )
        }
        (Violation::Range { .. }, Some(c)) => {
            let (lo, hi) = match c.range {
                Some(RangeRef::Int(a, b)) => (a.map(|x| x as f64), b.map(|x| x as f64)),
                Some(RangeRef::Float(a, b)) => (a, b),
                None => (None, None),
            };
            bound("value", value.and_then(|v| v.parse().ok()), lo, hi)
        }
        _ => "satisfy its constraint".to_owned(),
    };
    let mut name = m.name.to_owned();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_lowercase();
    }
    format!(
        "1 validation error detected: Value '{}' at '{name}' failed to satisfy constraint: Member must {must}",
        value.unwrap_or("null")
    )
}

/// `have length less than or equal to 10`: the bound the value broke (the upper one when it is not known which).
#[allow(clippy::cast_precision_loss)]
fn bound(what: &str, actual: Option<f64>, lo: Option<f64>, hi: Option<f64>) -> String {
    let below = lo.filter(|lo| actual.is_some_and(|a| a < *lo));
    match (below, hi) {
        (Some(lo), _) => format!("have {what} greater than or equal to {lo}"),
        (None, Some(hi)) => format!("have {what} less than or equal to {hi}"),
        (None, None) => format!("have a valid {what}"),
    }
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
