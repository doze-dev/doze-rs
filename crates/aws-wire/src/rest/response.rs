//! A REST answer written from the JSON a service produced for the operation's output: header-bound members become headers, a
//! prefix-bound map becomes a family of headers, the status member sets the status, the payload member is the body (raw, or XML for
//! a structure), and the rest of the members are the XML body under the output structure's root element.

use aws_core::time::format_http_date;
use aws_schema::tables::{K_BLOB, K_STR, K_STREAM, K_STRUCT, K_TS, MemberRef, Tables};
use serde_json::Value;

use crate::query::xml::{element, escape, members};
use crate::{ApiError, RequestId};

/// An answer, ready to send.
#[derive(Debug, Default)]
pub struct Rendered {
    /// The status.
    pub status: u16,
    /// The headers, in the order written.
    pub headers: Vec<(String, String)>,
    /// The body (empty when there is none or when `streams` is set).
    pub body: Vec<u8>,
    /// The body is a stream the service supplies (a streaming blob payload).
    pub streams: bool,
}

/// The answer for operation `op` carrying `out`, with `status` instead of the model's if given.
#[must_use]
pub fn render(
    tables: &Tables,
    namespace: Option<&str>,
    op: u32,
    out: &Value,
    status: Option<u16>,
) -> Rendered {
    let mut r = Rendered::default();
    let Some(o) = tables.op(op) else { return r };
    r.status = status.unwrap_or(o.code);
    let Some(sh) = o.output.and_then(|s| tables.shape(s).map(|sh| (s, sh))) else {
        return r;
    };
    let (shape, sh) = sh;
    let ms: Vec<MemberRef<'_>> = (sh.first..sh.first + sh.count)
        .filter_map(|i| tables.member(i))
        .collect();
    let mut body_members = false;
    for m in &ms {
        let value = out.get(m.wire);
        match m.bind {
            4 => header(&mut r, m, value),
            5 => prefixed(&mut r, m, value),
            7 => {
                if let Some(code) = value
                    .and_then(Value::as_u64)
                    .and_then(|c| u16::try_from(c).ok())
                {
                    r.status = code;
                }
            }
            6 => payload(tables, namespace, &mut r, m, value),
            0 => body_members = true,
            _ => {}
        }
    }
    if body_members && !ms.iter().any(|m| m.bind == 6) {
        let root = sh.xml.unwrap_or(sh.name);
        r.body = document(tables, namespace, root, shape, out).into_bytes();
    }
    r
}

fn header(r: &mut Rendered, m: &MemberRef<'_>, value: Option<&Value>) {
    let (Some(name), Some(v)) = (m.bind_str, value) else {
        return;
    };
    if let Some(text) = header_text(m, v) {
        r.headers.push((name.to_owned(), text));
    }
}

fn header_text(m: &MemberRef<'_>, v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) if m.kind == K_TS => {
            let secs = n.as_f64()?.floor();
            #[allow(clippy::cast_possible_truncation)]
            let secs = secs as i64;
            Some(if m.ts == 1 {
                n.to_string()
            } else {
                format_http_date(secs)
            })
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        Value::Null | Value::Object(_) => None,
    }
}

fn prefixed(r: &mut Rendered, m: &MemberRef<'_>, value: Option<&Value>) {
    let (Some(prefix), Some(Value::Object(map))) = (m.bind_str, value) else {
        return;
    };
    for (k, v) in map {
        if let Some(s) = v.as_str() {
            r.headers.push((format!("{prefix}{k}"), s.to_owned()));
        }
    }
}

fn payload(
    tables: &Tables,
    namespace: Option<&str>,
    r: &mut Rendered,
    m: &MemberRef<'_>,
    value: Option<&Value>,
) {
    match (m.kind, value) {
        (K_STREAM, _) => r.streams = true,
        (K_STR, Some(Value::String(s))) => r.body = s.clone().into_bytes(),
        (K_BLOB, Some(Value::String(s))) => {
            r.body = aws_core::base64::decode(s).unwrap_or_default()
        }
        (K_STRUCT, Some(v)) => {
            let root = m.xml.unwrap_or(m.name);
            r.body = document(tables, namespace, root, m.target, v).into_bytes();
        }
        _ => {}
    }
}

/// `<Root xmlns="…">` members of structure `shape` in `v` `</Root>`. A root that holds one text member of the same name as itself
/// (`LocationConstraint`) holds the text directly.
fn document(tables: &Tables, namespace: Option<&str>, root: &str, shape: u32, v: &Value) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<");
    out.push_str(root);
    if let Some(ns) = namespace {
        out.push_str(" xmlns=\"");
        escape(&mut out, ns);
        out.push('"');
    }
    out.push('>');
    let sole_text = tables
        .shape(shape)
        .filter(|s| s.count == 1)
        .and_then(|s| tables.member(s.first));
    match sole_text.filter(|m| m.name == root && m.kind == K_STR) {
        Some(m) => {
            if let Some(Value::String(s)) = v.get(m.wire) {
                escape(&mut out, s);
            }
        }
        None => members(tables, shape, v, &mut out, 0),
    }
    out.push_str("</");
    out.push_str(root);
    out.push('>');
    out
}

/// The error answer S3 gives: `<Error><Code>…</Code><Message>…</Message>` and any fields the error carries (`Key`, `BucketName`),
/// then the request id and host id. A HEAD answer has no body.
#[must_use]
pub fn error(err: &ApiError, id: &RequestId, head: bool) -> Rendered {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    element(&mut out, "Error", |o| {
        element(o, "Code", |x| escape(x, &err.code));
        element(o, "Message", |x| escape(x, &err.message));
        for (k, v) in &err.extra {
            element(o, k, |x| escape(x, v));
        }
        element(o, "RequestId", |x| escape(x, id.as_str()));
        element(o, "HostId", |x| escape(x, &host_id(id)));
    });
    Rendered {
        status: err.status,
        headers: vec![("content-type".to_owned(), "application/xml".to_owned())],
        body: if head { Vec::new() } else { out.into_bytes() },
        streams: false,
    }
}

/// The `x-amz-id-2` of an answer: S3 returns an opaque token; ours is derived from the request id.
#[must_use]
pub fn host_id(id: &RequestId) -> String {
    aws_core::base64::encode(id.as_str().as_bytes())
}

#[cfg(test)]
mod tests {
    use aws_model_s3::meta::META;
    use aws_model_s3::tables::TABLES;
    use serde_json::json;

    use super::*;

    fn op(name: &str) -> u32 {
        TABLES.find_op(name).unwrap()
    }

    fn text(r: &Rendered) -> String {
        String::from_utf8(r.body.clone()).unwrap()
    }

    #[test]
    fn a_listing_is_xml_under_the_models_root_and_a_header_is_a_header() {
        let out = json!({"Name": "b", "KeyCount": 1, "IsTruncated": false,
            "Contents": [{"Key": "a&b", "Size": 3, "LastModified": 1_700_000_000}]});
        let r = render(&TABLES, META.xml_namespace, op("ListObjectsV2"), &out, None);
        assert_eq!(r.status, 200);
        let body = text(&r);
        assert!(body.contains("<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><IsTruncated>false</IsTruncated>"), "{body}");
        assert!(
            body.contains(
                "<Contents><Key>a&amp;b</Key><LastModified>2023-11-14T22:13:20.000Z</LastModified>"
            ),
            "{body}"
        );

        let head = json!({"ETag": "\"x\"", "ContentLength": 5, "Metadata": {"origin": "here"}, "LastModified": 1_700_000_000});
        let r = render(&TABLES, META.xml_namespace, op("HeadObject"), &head, None);
        let get = |n: &str| {
            r.headers
                .iter()
                .find(|(k, _)| k == n)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("ETag"), Some("\"x\""));
        assert_eq!(get("Content-Length"), Some("5"));
        assert_eq!(get("x-amz-meta-origin"), Some("here"));
        assert_eq!(get("Last-Modified"), Some("Tue, 14 Nov 2023 22:13:20 GMT"));
        assert!(r.body.is_empty());
    }

    #[test]
    fn a_string_payload_is_raw_and_a_structure_payload_has_its_own_root() {
        let r = render(
            &TABLES,
            META.xml_namespace,
            op("GetBucketPolicy"),
            &json!({"Policy": "{\"a\":1}"}),
            None,
        );
        assert_eq!(text(&r), "{\"a\":1}");
        let out = json!({"PublicAccessBlockConfiguration": {"BlockPublicAcls": true}});
        let r = render(
            &TABLES,
            META.xml_namespace,
            op("GetPublicAccessBlock"),
            &out,
            None,
        );
        assert!(
            text(&r).contains("<PublicAccessBlockConfiguration xmlns="),
            "{}",
            text(&r)
        );
        assert!(text(&r).contains("<BlockPublicAcls>true</BlockPublicAcls>"));
    }

    #[test]
    fn the_location_constraint_is_the_text_of_the_root() {
        let r = render(
            &TABLES,
            META.xml_namespace,
            op("GetBucketLocation"),
            &json!({"LocationConstraint": "eu-west-1"}),
            None,
        );
        assert!(text(&r).ends_with("<LocationConstraint xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">eu-west-1</LocationConstraint>"), "{}", text(&r));
    }

    #[test]
    fn an_error_names_its_code_and_extra_fields() {
        let id = RequestId::new();
        let e =
            ApiError::new(404, "NoSuchKey", "The specified key does not exist.").with("Key", "k");
        let r = error(&e, &id, false);
        assert_eq!(r.status, 404);
        assert!(text(&r).contains("<Error><Code>NoSuchKey</Code><Message>The specified key does not exist.</Message><Key>k</Key><RequestId>"), "{}", text(&r));
        assert!(error(&e, &id, true).body.is_empty());
    }
}
