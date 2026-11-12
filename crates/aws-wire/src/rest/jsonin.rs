//! A restJson1 request read into the JSON the model's decoder takes: path labels, query parameters, headers and header prefixes come
//! from where the model binds them, and the members that have no binding are the members of the JSON body. The decoder then checks
//! the result as it checks any request, so a REST-JSON request is held to the same rules as an awsJson one.
//!
//! A member bound as the payload takes the whole body: a blob as its bytes (base64 in the JSON the decoder reads), a string as its
//! text, a structure as the body's JSON object. Nesting is capped by `serde_json` (128 levels), so a hostile body cannot overflow
//! the stack.

use aws_schema::tables::{K_BLOB, K_STR, K_STREAM, K_STRUCT, K_UNION, MemberRef, Tables};
use serde_json::{Map, Value};

use super::request::{Parts, header, prefixed, query, scalar};

/// What stops a request from being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The body is not a JSON object (or not JSON at all).
    MalformedJson,
}

// fuzz:parser fuzz_restjson
/// The input of the operation whose input structure is `shape`, as JSON. A streaming blob is left out: the service reads that
/// stream itself.
///
/// # Errors
/// [`ReadError::MalformedJson`] for a body that is not a JSON object where the model wants one.
pub fn to_json(tables: &Tables, shape: u32, p: &Parts<'_>) -> Result<Value, ReadError> {
    let Some(sh) = tables.shape(shape) else {
        return Ok(Value::Object(Map::new()));
    };
    let members: Vec<(u32, MemberRef<'_>)> = (sh.first..sh.first + sh.count)
        .filter_map(|i| tables.member(i).map(|m| (i, m)))
        .collect();
    let has_payload = members.iter().any(|(_, m)| m.bind == 6);
    let wants_object = members.iter().any(|(_, m)| m.bind == 0)
        || members
            .iter()
            .any(|(_, m)| m.bind == 6 && matches!(m.kind, K_STRUCT | K_UNION));
    let body = if wants_object && !is_blank(p.body) {
        match serde_json::from_slice::<Value>(p.body) {
            Ok(Value::Object(map)) => Some(map),
            _ => return Err(ReadError::MalformedJson),
        }
    } else {
        None
    };
    let mut out = Map::new();
    // Body members first, so a bound member (a label, a header) of the same name is the bound one.
    if let Some(map) = &body
        && !has_payload
    {
        for (_, m) in members.iter().filter(|(_, m)| m.bind == 0) {
            if let Some(v) = map.get(m.wire) {
                out.insert(m.wire.to_owned(), v.clone());
            }
        }
    }
    for (ix, m) in &members {
        let value = match m.bind {
            1 => p
                .labels
                .iter()
                .find(|(l, _)| l == ix)
                .map(|(_, v)| scalar(m.kind, v)),
            2 => query(m, p.query),
            4 => header(m, p.headers),
            5 => prefixed(m, p.headers),
            6 => payload(m, p.body, body.as_ref()),
            _ => None,
        };
        if let Some(v) = value {
            out.insert(m.wire.to_owned(), v);
        }
    }
    Ok(Value::Object(out))
}

fn is_blank(body: &[u8]) -> bool {
    body.iter().all(u8::is_ascii_whitespace)
}

fn payload(m: &MemberRef<'_>, body: &[u8], object: Option<&Map<String, Value>>) -> Option<Value> {
    match m.kind {
        K_STREAM => None,
        K_STR => {
            (!body.is_empty()).then(|| Value::String(String::from_utf8_lossy(body).into_owned()))
        }
        K_BLOB => (!body.is_empty()).then(|| Value::String(aws_core::base64::encode(body))),
        K_STRUCT | K_UNION => object.map(|o| Value::Object(o.clone())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_model_lambda::tables::TABLES;
    use http::{HeaderMap, HeaderValue};

    use super::*;

    fn input(op: &str) -> u32 {
        TABLES
            .op(TABLES.find_op(op).unwrap())
            .unwrap()
            .input
            .unwrap()
    }

    fn member_of(op: &str, name: &str) -> u32 {
        let sh = TABLES.shape(input(op)).unwrap();
        (sh.first..sh.first + sh.count)
            .find(|i| TABLES.member(*i).unwrap().name == name)
            .unwrap()
    }

    #[test]
    fn labels_query_headers_and_body_members_meet_in_one_object() {
        let mut headers = HeaderMap::new();
        headers.insert("x-amz-invocation-type", HeaderValue::from_static("Event"));
        let labels = [(member_of("Invoke", "FunctionName"), "f".to_owned())];
        let q = [("Qualifier".to_owned(), "live".to_owned())];
        let p = Parts {
            labels: &labels,
            query: &q,
            headers: &headers,
            body: b"{\"a\":1}",
        };
        let v = to_json(&TABLES, input("Invoke"), &p).unwrap();
        assert_eq!(v["FunctionName"], "f");
        assert_eq!(v["Qualifier"], "live");
        assert_eq!(v["InvocationType"], "Event");
        // The payload is the body, as base64 for the decoder.
        assert_eq!(v["Payload"], aws_core::base64::encode(b"{\"a\":1}"));

        let labels = [(member_of("CreateAlias", "FunctionName"), "f".to_owned())];
        let p = Parts {
            labels: &labels,
            query: &[],
            headers: &headers,
            body: b"{\"Name\":\"live\",\"FunctionVersion\":\"1\",\"RoutingConfig\":{\"AdditionalVersionWeights\":{\"2\":0.1}}}",
        };
        let v = to_json(&TABLES, input("CreateAlias"), &p).unwrap();
        assert_eq!(v["Name"], "live");
        assert_eq!(v["RoutingConfig"]["AdditionalVersionWeights"]["2"], 0.1);
        let bad = Parts { body: b"[1]", ..p };
        assert_eq!(
            to_json(&TABLES, input("CreateAlias"), &bad),
            Err(ReadError::MalformedJson)
        );
    }

    #[test]
    fn an_empty_body_is_no_members() {
        let labels = [(member_of("GetFunction", "FunctionName"), "f".to_owned())];
        let p = Parts {
            labels: &labels,
            query: &[],
            headers: &HeaderMap::new(),
            body: b"",
        };
        let v = to_json(&TABLES, input("GetFunction"), &p).unwrap();
        assert_eq!(v, serde_json::json!({"FunctionName": "f"}));
    }
}
