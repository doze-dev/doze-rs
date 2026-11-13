//! A REST request read into the JSON the model's decoder takes: path labels, query parameters, headers, header prefixes and an XML
//! (or plain) body, each into the input member the model binds it to. The decoder then checks it as it checks any request, so a
//! REST request is held to the same rules as a JSON one.

use aws_schema::tables::{
    K_BLOB, K_BOOL, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STREAM, K_STRUCT, K_UNION, MemberRef,
    Tables,
};
use http::HeaderMap;
use serde_json::{Map, Number, Value};

use super::xmlin::{Elem, XmlError, parse};

/// Where each part of the request is.
#[derive(Debug)]
pub struct Parts<'a> {
    /// The path labels: (input member index, decoded value).
    pub labels: &'a [(u32, String)],
    /// The query pairs, decoded, in order.
    pub query: &'a [(String, String)],
    /// The headers.
    pub headers: &'a HeaderMap,
    /// The body (empty when there is none, or when it is a stream the service reads itself).
    pub body: &'a [u8],
}

/// What stops a request from being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The body is not XML this reader accepts.
    MalformedXml,
}

impl From<XmlError> for ReadError {
    fn from(_: XmlError) -> Self {
        Self::MalformedXml
    }
}

/// The input of the operation whose input structure is `shape`, as JSON. A streaming body member is left out: the service reads
/// that stream itself.
///
/// # Errors
/// [`ReadError::MalformedXml`] for a body that is not XML.
pub fn to_json(tables: &Tables, shape: u32, p: &Parts<'_>) -> Result<Value, ReadError> {
    let Some(sh) = tables.shape(shape) else {
        return Ok(Value::Object(Map::new()));
    };
    let members: Vec<(u32, MemberRef<'_>)> = (sh.first..sh.first + sh.count)
        .filter_map(|i| tables.member(i).map(|m| (i, m)))
        .collect();
    let mut out = Map::new();
    let xml = if members.iter().any(|(_, m)| m.bind == 0) && !p.body.is_empty() {
        Some(parse(p.body)?)
    } else {
        None
    };
    for (ix, m) in &members {
        let value = match m.bind {
            1 => p
                .labels
                .iter()
                .find(|(l, _)| l == ix)
                .map(|(_, v)| Value::String(v.clone())),
            2 => query(m, p.query),
            3 => None,
            4 => header(m, p.headers),
            5 => prefixed(m, p.headers),
            6 => payload(tables, m, p.body)?,
            0 => xml.as_ref().and_then(|e| child(tables, m, e)),
            _ => None,
        };
        if let Some(v) = value {
            out.insert(m.wire.to_owned(), v);
        }
    }
    Ok(Value::Object(out))
}

pub(super) fn query(m: &MemberRef<'_>, pairs: &[(String, String)]) -> Option<Value> {
    let name = m.bind_str?;
    let mut hits = pairs
        .iter()
        .filter(|(k, _)| k == name)
        .map(|(_, v)| v.as_str());
    if m.kind == K_LIST {
        let items: Vec<Value> = hits.map(|v| Value::String(v.to_owned())).collect();
        return (!items.is_empty()).then_some(Value::Array(items));
    }
    hits.next().map(|v| scalar(m.kind, v))
}

pub(super) fn header(m: &MemberRef<'_>, headers: &HeaderMap) -> Option<Value> {
    let name = m.bind_str?;
    let values: Vec<&str> = headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    if m.kind == K_LIST {
        let items: Vec<Value> = values
            .iter()
            .flat_map(|v| v.split(','))
            .map(|v| Value::String(v.trim().to_owned()))
            .collect();
        return (!items.is_empty()).then_some(Value::Array(items));
    }
    values.first().map(|v| scalar(m.kind, v))
}

/// Every header that starts with the member's prefix, as a map of the rest of its name.
pub(super) fn prefixed(m: &MemberRef<'_>, headers: &HeaderMap) -> Option<Value> {
    let prefix = m.bind_str?.to_ascii_lowercase();
    let map: Map<String, Value> = headers
        .iter()
        .filter_map(|(k, v)| {
            let rest = k.as_str().strip_prefix(prefix.as_str())?;
            Some((rest.to_owned(), Value::String(v.to_str().ok()?.to_owned())))
        })
        .collect();
    (!map.is_empty()).then_some(Value::Object(map))
}

fn payload(tables: &Tables, m: &MemberRef<'_>, body: &[u8]) -> Result<Option<Value>, ReadError> {
    match m.kind {
        K_STREAM => Ok(None),
        K_STR => {
            Ok((!body.is_empty())
                .then(|| Value::String(String::from_utf8_lossy(body).into_owned())))
        }
        K_BLOB => Ok((!body.is_empty()).then(|| Value::String(aws_core::base64::encode(body)))),
        K_STRUCT | K_UNION if !body.is_empty() => {
            let root = parse(body)?;
            Ok(Some(from_elem(tables, m, &root)))
        }
        _ => Ok(None),
    }
}

/// The member of a body-bound structure that is in the XML root.
fn child(tables: &Tables, m: &MemberRef<'_>, root: &Elem) -> Option<Value> {
    let name = m.xml.unwrap_or(m.name);
    if m.kind == K_LIST && m.flags & 2 != 0 {
        let element = tables.member(m.target)?;
        let items: Vec<Value> = root
            .children
            .iter()
            .filter(|c| c.name == name)
            .map(|c| from_elem(tables, &element, c))
            .collect();
        return (!items.is_empty()).then_some(Value::Array(items));
    }
    root.children
        .iter()
        .find(|c| c.name == name)
        .map(|c| from_elem(tables, m, c))
}

/// The value of kind `m` held by element `e`.
fn from_elem(tables: &Tables, m: &MemberRef<'_>, e: &Elem) -> Value {
    match m.kind {
        K_STRUCT | K_UNION => Value::Object(object(tables, m.target, e)),
        K_LIST => {
            let Some(element) = tables.member(m.target) else {
                return Value::Array(Vec::new());
            };
            let tag = element.xml.unwrap_or("member");
            Value::Array(
                e.children
                    .iter()
                    .filter(|c| c.name == tag)
                    .map(|c| from_elem(tables, &element, c))
                    .collect(),
            )
        }
        K_MAP => Value::Object(entries(tables, m, e)),
        kind => scalar(kind, &e.text),
    }
}

fn entries(tables: &Tables, m: &MemberRef<'_>, e: &Elem) -> Map<String, Value> {
    let (Some(k), Some(v)) = (tables.member(m.target), tables.member(m.target + 1)) else {
        return Map::new();
    };
    e.children
        .iter()
        .filter(|c| c.name == "entry")
        .filter_map(|c| {
            let key = c
                .children
                .iter()
                .find(|x| x.name == k.xml.unwrap_or("key"))?;
            let val = c
                .children
                .iter()
                .find(|x| x.name == v.xml.unwrap_or("value"))?;
            Some((key.text.clone(), from_elem(tables, &v, val)))
        })
        .collect()
}

fn object(tables: &Tables, shape: u32, e: &Elem) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(sh) = tables.shape(shape) else {
        return out;
    };
    for i in sh.first..sh.first + sh.count {
        let Some(m) = tables.member(i) else { continue };
        if let Some(v) = child(tables, &m, e) {
            out.insert(m.wire.to_owned(), v);
        }
    }
    out
}

/// Text read as the kind of member it is bound to; text that does not fit stays text, so the decoder reports the wrong type.
pub(super) fn scalar(kind: u8, text: &str) -> Value {
    match kind {
        K_BOOL => match text {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::String(text.to_owned()),
        },
        K_INT => text.parse::<i64>().map_or_else(
            |_| Value::String(text.to_owned()),
            |n| Value::Number(n.into()),
        ),
        K_FLOAT => text
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map_or_else(|| Value::String(text.to_owned()), Value::Number),
        _ => Value::String(text.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use aws_model_s3::tables::TABLES;
    use http::HeaderValue;

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
    fn labels_headers_query_and_prefix_headers_meet_in_one_object() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("text/plain"));
        headers.insert("x-amz-meta-origin", HeaderValue::from_static("here"));
        headers.insert("if-match", HeaderValue::from_static("\"e\""));
        let labels = [
            (member_of("PutObject", "Bucket"), "b".to_owned()),
            (member_of("PutObject", "Key"), "k".to_owned()),
        ];
        let p = Parts {
            labels: &labels,
            query: &[],
            headers: &headers,
            body: b"",
        };
        let v = to_json(&TABLES, input("PutObject"), &p).unwrap();
        assert_eq!(v["Bucket"], "b");
        assert_eq!(v["ContentType"], "text/plain");
        assert_eq!(v["Metadata"]["origin"], "here");
        assert!(
            v.get("Body").is_none(),
            "the stream is the service's to read"
        );

        let q = [
            ("max-keys".to_owned(), "5".to_owned()),
            ("prefix".to_owned(), "a/".to_owned()),
        ];
        let labels = [(member_of("ListObjectsV2", "Bucket"), "b".to_owned())];
        let p = Parts {
            labels: &labels,
            query: &q,
            headers: &headers,
            body: b"",
        };
        let v = to_json(&TABLES, input("ListObjectsV2"), &p).unwrap();
        assert_eq!(
            (v["MaxKeys"].as_i64(), v["Prefix"].as_str()),
            (Some(5), Some("a/"))
        );
    }

    #[test]
    fn an_xml_payload_becomes_the_member_it_names() {
        let body = b"<Delete><Quiet>true</Quiet><Object><Key>a</Key></Object><Object><Key>b</Key><VersionId>1</VersionId></Object></Delete>";
        let labels = [(member_of("DeleteObjects", "Bucket"), "b".to_owned())];
        let p = Parts {
            labels: &labels,
            query: &[],
            headers: &HeaderMap::new(),
            body,
        };
        let v = to_json(&TABLES, input("DeleteObjects"), &p).unwrap();
        assert_eq!(v["Delete"]["Quiet"], true);
        assert_eq!(v["Delete"]["Objects"][1]["VersionId"], "1");
        let bad = Parts {
            body: b"<Delete",
            ..p
        };
        assert_eq!(
            to_json(&TABLES, input("DeleteObjects"), &bad),
            Err(ReadError::MalformedXml)
        );
    }

    #[test]
    fn a_string_payload_is_the_body() {
        let labels = [(member_of("PutBucketPolicy", "Bucket"), "b".to_owned())];
        let p = Parts {
            labels: &labels,
            query: &[],
            headers: &HeaderMap::new(),
            body: b"{\"a\":1}",
        };
        let v = to_json(&TABLES, input("PutBucketPolicy"), &p).unwrap();
        assert_eq!(v["Policy"], "{\"a\":1}");
    }
}
