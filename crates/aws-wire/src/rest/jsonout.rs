//! A restJson1 answer written from the JSON a service produced for the operation's output: header-bound members become headers, a
//! prefix-bound map a family of headers, the response-code member the status, the payload member the body (raw bytes for a blob,
//! text for a string, JSON for a structure) and the members with no binding the members of a JSON object body.
//!
//! Errors are Lambda's and API Gateway's: the code travels in `x-amzn-ErrorType`, the body is `{"Type":"User","Message":"…"}` for
//! an error a service raises and `{"message":"…"}` for one the front end raises (validation, authorization).

use aws_schema::tables::{K_BLOB, K_EVENT, K_STR, K_STREAM, K_STRUCT, MemberRef, Tables};
use serde_json::{Map, Value};

use super::response::{Rendered, header, prefixed};
use crate::ApiError;

/// The answer for operation `op` carrying `out`, with `status` instead of the model's if given.
#[must_use]
pub fn render(tables: &Tables, op: u32, out: &Value, status: Option<u16>) -> Rendered {
    let mut r = Rendered::default();
    let Some(o) = tables.op(op) else { return r };
    r.status = status.unwrap_or(o.code);
    let Some(sh) = o.output.and_then(|s| tables.shape(s)) else {
        return r;
    };
    let ms: Vec<MemberRef<'_>> = (sh.first..sh.first + sh.count)
        .filter_map(|i| tables.member(i))
        .collect();
    let mut object = Map::new();
    let mut has_payload = false;
    let mut has_body = false;
    for m in &ms {
        let value = out.get(m.wire);
        match m.bind {
            4 => header(&mut r, m, value),
            5 => prefixed(&mut r, m, value),
            7 => {
                if let Some(code) = value
                    .and_then(Value::as_u64)
                    .and_then(|c| u16::try_from(c).ok())
                    .filter(|c| *c != 0)
                {
                    r.status = code;
                }
            }
            6 => {
                has_payload = true;
                payload(&mut r, m, value);
            }
            0 => {
                has_body = true;
                if let Some(v) = value.filter(|v| !v.is_null()) {
                    object.insert(m.wire.to_owned(), v.clone());
                }
            }
            _ => {}
        }
    }
    if has_body && !has_payload && !matches!(r.status, 204 | 304) {
        r.body = Value::Object(object).to_string().into_bytes();
    }
    if !r.body.is_empty() || r.streams {
        let has_type = r
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
        if !has_type {
            r.headers
                .push(("content-type".to_owned(), "application/json".to_owned()));
        }
    }
    r
}

fn payload(r: &mut Rendered, m: &MemberRef<'_>, value: Option<&Value>) {
    match (m.kind, value) {
        (K_STREAM | K_EVENT, _) => r.streams = true,
        (K_STR, Some(Value::String(s))) => r.body = s.clone().into_bytes(),
        (K_BLOB, Some(Value::String(s))) => {
            r.body = aws_core::base64::decode(s).unwrap_or_default()
        }
        (K_STRUCT, Some(v)) => r.body = v.to_string().into_bytes(),
        _ => {}
    }
}

/// The codes the front end raises itself, whose body words the message in lower case.
fn front_end(code: &str) -> bool {
    matches!(
        code,
        "ValidationException"
            | "AccessDeniedException"
            | "UnrecognizedClientException"
            | "InvalidSignatureException"
            | "IncompleteSignatureException"
            | "MissingAuthenticationTokenException"
            | "ExpiredTokenException"
            | "RequestEntityTooLargeException"
            | "UnknownOperationException"
            | "NotImplemented"
    )
}

/// The error answer: the code in `x-amzn-ErrorType`, the words in the body.
#[must_use]
pub fn error(err: &ApiError) -> Rendered {
    let mut body = Map::new();
    if front_end(&err.code) {
        body.insert("message".to_owned(), Value::String(err.message.clone()));
    } else {
        let kind = if err.status >= 500 { "Service" } else { "User" };
        body.insert("Type".to_owned(), Value::String(kind.to_owned()));
        body.insert("Message".to_owned(), Value::String(err.message.clone()));
    }
    for (k, v) in &err.extra {
        body.insert(k.clone(), Value::String(v.clone()));
    }
    Rendered {
        status: err.status,
        headers: vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            ("x-amzn-ErrorType".to_owned(), err.code.to_string()),
        ],
        body: Value::Object(body).to_string().into_bytes(),
        streams: false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_model_lambda::tables::TABLES;
    use serde_json::json;

    use super::*;

    fn op(name: &str) -> u32 {
        TABLES.find_op(name).unwrap()
    }

    fn header_of<'a>(r: &'a Rendered, name: &str) -> Option<&'a str> {
        r.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn an_invoke_answer_is_headers_a_status_and_the_payload_bytes() {
        let out = json!({"StatusCode": 200, "FunctionError": "Unhandled", "ExecutedVersion": "$LATEST",
            "Payload": aws_core::base64::encode(b"{\"a\":1}")});
        let r = render(&TABLES, op("Invoke"), &out, None);
        assert_eq!(
            (r.status, r.body.as_slice()),
            (200, b"{\"a\":1}".as_slice())
        );
        assert_eq!(header_of(&r, "X-Amz-Function-Error"), Some("Unhandled"));
        assert_eq!(header_of(&r, "X-Amz-Executed-Version"), Some("$LATEST"));
        let dry = render(&TABLES, op("Invoke"), &json!({"StatusCode": 204}), None);
        assert_eq!((dry.status, dry.body.len()), (204, 0));
    }

    #[test]
    fn a_structure_answer_is_a_json_object_and_the_model_gives_the_status() {
        let out = json!({"FunctionName": "f", "Version": "$LATEST", "Gone": null});
        let r = render(&TABLES, op("CreateFunction"), &out, None);
        assert_eq!(r.status, 201);
        let body: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(body, json!({"FunctionName": "f", "Version": "$LATEST"}));
        assert_eq!(header_of(&r, "content-type"), Some("application/json"));
        let none = render(&TABLES, op("DeleteAlias"), &json!({}), None);
        assert_eq!((none.status, none.body.len()), (204, 0));
    }

    #[test]
    fn an_error_names_its_code_in_a_header() {
        let e = ApiError::new(404, "ResourceNotFoundException", "Function not found: x");
        let r = error(&e);
        assert_eq!(
            header_of(&r, "x-amzn-ErrorType"),
            Some("ResourceNotFoundException")
        );
        let body: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(
            body,
            json!({"Type": "User", "Message": "Function not found: x"})
        );
        let v = error(&ApiError::new(400, "ValidationException", "bad"));
        assert_eq!(
            serde_json::from_slice::<Value>(&v.body).unwrap(),
            json!({"message": "bad"})
        );
    }
}
