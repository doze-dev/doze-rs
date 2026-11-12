//! The awsJson1.0 and awsJson1.1 protocols: a POST to `/`, the operation named in `X-Amz-Target`, JSON in and out.
//!
//! Decoding the body is `aws_schema::json`; this module is everything around it: finding the operation, and wording the
//! answer and the errors.

use aws_schema::meta::{Protocol, ServiceMeta};
use aws_schema::tables::Tables;
use http::request::Parts;
use http::{Response, StatusCode, header};

use crate::{ApiError, Fault, RequestId};

/// 1.0 or 1.1: they differ only in the content type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    /// `awsJson1_0`.
    V10,
    /// `awsJson1_1`.
    V11,
}

impl Version {
    /// The version a service speaks, if it speaks a JSON protocol.
    #[must_use]
    pub fn of(meta: &ServiceMeta) -> Option<Self> {
        if meta.speaks(Protocol::AwsJson10) {
            Some(Self::V10)
        } else if meta.speaks(Protocol::AwsJson11) {
            Some(Self::V11)
        } else {
            None
        }
    }

    /// The `Content-Type` of requests and answers.
    #[must_use]
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::V10 => "application/x-amz-json-1.0",
            Self::V11 => "application/x-amz-json-1.1",
        }
    }
}

/// The operation a request asks for: an index into the model's tables.
///
/// # Errors
/// `UnknownOperationException` when the request is not a POST to `/`, names no target, or names another service's or an
/// operation the model does not have. (Unverified against AWS: recorded for the first service that needs it.)
pub fn route(meta: &ServiceMeta, tables: &Tables, parts: &Parts) -> Result<u32, ApiError> {
    let unknown = || ApiError::new(404, "UnknownOperationException", "");
    if parts.method != http::Method::POST || parts.uri.path() != "/" {
        return Err(unknown());
    }
    let target = parts
        .headers
        .get("x-amz-target")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(unknown)?;
    let (prefix, operation) = target.split_once('.').ok_or_else(unknown)?;
    if prefix != meta.shape_name {
        return Err(unknown());
    }
    tables.find_op(operation).ok_or_else(unknown)
}

/// The 200 answer for a body already encoded.
#[must_use]
pub fn success(version: Version, id: &RequestId, body: Vec<u8>) -> Response<Vec<u8>> {
    base(StatusCode::OK, version, id)
        .body(body)
        .unwrap_or_default()
}

/// The answer for `err`. A modelled error says its code, status and fault in the model; any other states them itself.
#[must_use]
pub fn failure(
    meta: &ServiceMeta,
    tables: &Tables,
    version: Version,
    id: &RequestId,
    err: &ApiError,
) -> Response<Vec<u8>> {
    let modeled = err.shape.as_deref().and_then(|name| lookup(tables, name));
    let (status, fault, wire_code, query_code) = match &modeled {
        Some(m) => (
            m.status,
            m.fault,
            m.name.as_str(),
            Some(m.query_code.as_str()),
        ),
        None => (err.status, err.fault, err.code.as_ref(), None),
    };
    let namespace = if modeled.is_some() {
        meta.namespace
    } else {
        "com.amazon.coral.service"
    };
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    let mut builder = base(status, version, id);
    if meta.query_compatible {
        let code = query_code.unwrap_or(err.code.as_ref());
        let who = if fault == Fault::Receiver {
            "Receiver"
        } else {
            "Sender"
        };
        builder = builder.header("x-amzn-query-error", format!("{code};{who}"));
    }
    let mut body = Vec::with_capacity(64 + err.message.len());
    body.extend_from_slice(br#"{"__type":"#);
    quote(&mut body, &format!("{namespace}#{wire_code}"));
    body.extend_from_slice(br#","message":"#);
    quote(&mut body, &err.message);
    body.push(b'}');
    builder.body(body).unwrap_or_default()
}

/// The answer for an error raised before any service is known (authentication, an unroutable request): the Coral front end's
/// own, with no model to consult.
#[must_use]
pub fn generic_failure(version: Version, id: &RequestId, err: &ApiError) -> Response<Vec<u8>> {
    let status = StatusCode::from_u16(err.status).unwrap_or(StatusCode::BAD_REQUEST);
    let mut body = Vec::with_capacity(64 + err.message.len());
    body.extend_from_slice(br#"{"__type":"#);
    quote(&mut body, &format!("com.amazon.coral.service#{}", err.code));
    body.extend_from_slice(br#","message":"#);
    quote(&mut body, &err.message);
    body.push(b'}');
    base(status, version, id).body(body).unwrap_or_default()
}

fn base(status: StatusCode, version: Version, id: &RequestId) -> http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, version.content_type())
        .header("x-amzn-RequestId", id.as_str())
}

/// What the model says about one error shape.
struct Modeled {
    name: String,
    query_code: String,
    status: u16,
    fault: Fault,
}

fn lookup(tables: &Tables, shape_name: &str) -> Option<Modeled> {
    (0..tables.n_errs())
        .filter_map(|i| tables.err(i))
        .find_map(|e| {
            let name = tables.shape(e.shape)?.name;
            (name == shape_name).then(|| Modeled {
                name: name.to_owned(),
                query_code: e.code.to_owned(),
                status: e.status,
                fault: if e.server {
                    Fault::Receiver
                } else {
                    Fault::Sender
                },
            })
        })
}

fn quote(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    for c in s.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes())
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use aws_model_sqs::meta::META;
    use aws_model_sqs::tables::TABLES;

    use super::*;

    fn parts(method: &str, uri: &str, target: Option<&str>) -> Parts {
        let mut b = http::Request::builder().method(method).uri(uri);
        if let Some(t) = target {
            b = b.header("x-amz-target", t);
        }
        b.body(()).unwrap().into_parts().0
    }

    #[test]
    fn the_target_names_the_operation() {
        let ix = route(
            &META,
            &TABLES,
            &parts("POST", "/", Some("AmazonSQS.SendMessage")),
        )
        .unwrap();
        assert_eq!(TABLES.op(ix).unwrap().name, "SendMessage");
    }

    #[test]
    fn anything_else_is_an_unknown_operation() {
        for p in [
            parts("POST", "/", None),
            parts("POST", "/", Some("AmazonSQS")),
            parts("POST", "/", Some("DynamoDB_20120810.PutItem")),
            parts("POST", "/", Some("AmazonSQS.NoSuchThing")),
            parts("GET", "/", Some("AmazonSQS.SendMessage")),
            parts("POST", "/x", Some("AmazonSQS.SendMessage")),
        ] {
            let e = route(&META, &TABLES, &p).unwrap_err();
            assert_eq!(
                (e.status, e.code.as_ref()),
                (404, "UnknownOperationException")
            );
        }
    }

    #[test]
    fn a_modelled_error_takes_its_status_and_codes_from_the_model() {
        let id = RequestId::new();
        let err = ApiError::modeled("QueueDoesNotExist", "The specified queue does not exist.");
        let r = failure(&META, &TABLES, Version::V10, &id, &err);
        assert_eq!(r.status(), 400);
        assert_eq!(r.headers()["content-type"], "application/x-amz-json-1.0");
        assert_eq!(r.headers()["x-amzn-requestid"], id.as_str());
        assert_eq!(
            r.headers()["x-amzn-query-error"],
            "AWS.SimpleQueueService.NonExistentQueue;Sender"
        );
        assert_eq!(
            String::from_utf8(r.into_body()).unwrap(),
            r#"{"__type":"com.amazonaws.sqs#QueueDoesNotExist","message":"The specified queue does not exist."}"#
        );
    }

    #[test]
    fn an_error_the_model_lacks_states_itself() {
        let err = ApiError::new(400, "InvalidSignatureException", "bad \"sig\"");
        let r = failure(&META, &TABLES, Version::V11, &RequestId::new(), &err);
        assert_eq!(r.status(), 400);
        assert_eq!(r.headers()["content-type"], "application/x-amz-json-1.1");
        // A query-compatible service names the code the Query protocol would, modelled or not.
        assert_eq!(
            r.headers()["x-amzn-query-error"],
            "InvalidSignatureException;Sender"
        );
        assert_eq!(
            String::from_utf8(r.into_body()).unwrap(),
            r#"{"__type":"com.amazon.coral.service#InvalidSignatureException","message":"bad \"sig\""}"#
        );
    }

    #[test]
    fn a_success_carries_the_id_and_the_body() {
        let id = RequestId::new();
        let r = success(Version::V10, &id, b"{}".to_vec());
        assert_eq!(
            (r.status().as_u16(), r.body().as_slice()),
            (200, &b"{}"[..])
        );
        assert_eq!(r.headers()["x-amzn-requestid"], id.as_str());
    }
}
