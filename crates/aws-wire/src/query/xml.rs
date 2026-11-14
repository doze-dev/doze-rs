//! The response half of awsQuery: the JSON a service's answer encodes to, written as the XML the Query protocol carries.
//!
//! `<OpResponse xmlns="…"><OpResult> members </OpResult><ResponseMetadata><RequestId>…</RequestId></ResponseMetadata></OpResponse>`;
//! a list is `<Name><member>…</member>…</Name>` (the element's `xmlName` instead of `member`), flattened as `<Name>…</Name>` repeated;
//! a map is `<Name><entry><key>…</key><value>…</value></entry></Name>`, flattened as `<Name><key>…</key><value>…</value></Name>`
//! repeated; a timestamp is ISO 8601.

use aws_core::time::format_iso8601;
use aws_schema::tables::{K_BLOB, K_LIST, K_MAP, K_STRUCT, K_TS, K_UNION, MemberRef, Tables};
use serde_json::Value;

const MAX_DEPTH: usize = 24;

/// The XML text of `s`, escaped.
pub(crate) fn escape(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
}

pub(crate) fn element(out: &mut String, name: &str, inner: impl FnOnce(&mut String)) {
    out.push('<');
    out.push_str(name);
    out.push('>');
    inner(out);
    out.push_str("</");
    out.push_str(name);
    out.push('>');
}

/// The members of structure `shape`, taken from the JSON object `v`, in the model's order.
pub(crate) fn members(t: &Tables, shape: u32, v: &Value, out: &mut String, depth: usize) {
    let (Some(sh), Some(obj)) = (t.shape(shape), v.as_object()) else {
        return;
    };
    if depth > MAX_DEPTH {
        return;
    }
    for i in sh.first..sh.first + sh.count {
        let Some(m) = t.member(i) else { continue };
        // An `@xmlAttribute` member is written in its structure's opening tag (`attributes`).
        if m.flags & 4 != 0 {
            continue;
        }
        if let Some(x) = obj.get(m.wire) {
            member(t, &m, x, out, depth + 1);
        }
    }
}

/// The `@xmlAttribute` members of structure `shape` in `v`, as ` name="value"`; an `xsi:` one brings its namespace declaration.
fn attributes(t: &Tables, shape: u32, v: &Value, out: &mut String) {
    let (Some(sh), Some(obj)) = (t.shape(shape), v.as_object()) else {
        return;
    };
    let mut declared = false;
    for i in sh.first..sh.first + sh.count {
        let Some(m) = t.member(i).filter(|m| m.flags & 4 != 0) else {
            continue;
        };
        let Some(text) = obj.get(m.wire).and_then(Value::as_str) else {
            continue;
        };
        let name = m.xml.unwrap_or(m.name);
        if name.starts_with("xsi:") && !declared {
            out.push_str(" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"");
            declared = true;
        }
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        escape(out, text);
        out.push('"');
    }
}

/// One member, under its own name (a flattened list repeats it).
fn member(t: &Tables, m: &MemberRef<'_>, v: &Value, out: &mut String, depth: usize) {
    let name = m.xml.unwrap_or(m.name);
    if m.kind == K_LIST && m.flags & 2 != 0 {
        if let (Some(items), Some(e)) = (v.as_array(), t.member(m.target)) {
            for item in items {
                tagged(t, &e, name, item, out, depth);
            }
        }
        return;
    }
    if m.kind == K_MAP && m.flags & 2 != 0 {
        if let (Some(entries), Some(k), Some(val)) =
            (v.as_object(), t.member(m.target), t.member(m.target + 1))
        {
            for (key, item) in entries {
                element(out, name, |o| pair(t, (&k, &val), key, item, o, depth));
            }
        }
        return;
    }
    tagged(t, m, name, v, out, depth);
}

fn pair(
    t: &Tables,
    (k, val): (&MemberRef<'_>, &MemberRef<'_>),
    key: &str,
    item: &Value,
    out: &mut String,
    depth: usize,
) {
    element(out, k.xml.unwrap_or("key"), |o| escape(o, key));
    tagged(t, val, val.xml.unwrap_or("value"), item, out, depth);
}

/// A value of kind `m` written inside a tag called `tag`.
fn tagged(t: &Tables, m: &MemberRef<'_>, tag: &str, v: &Value, out: &mut String, depth: usize) {
    match m.kind {
        K_STRUCT | K_UNION => {
            out.push('<');
            out.push_str(tag);
            attributes(t, m.target, v, out);
            out.push('>');
            members(t, m.target, v, out, depth);
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        K_LIST => element(out, tag, |o| {
            if let (Some(items), Some(e)) = (v.as_array(), t.member(m.target)) {
                for item in items {
                    tagged(t, &e, e.xml.unwrap_or("member"), item, o, depth + 1);
                }
            }
        }),
        K_MAP => element(out, tag, |o| {
            if let (Some(entries), Some(k), Some(val)) =
                (v.as_object(), t.member(m.target), t.member(m.target + 1))
            {
                for (key, item) in entries {
                    element(o, "entry", |e| pair(t, (&k, &val), key, item, e, depth + 1));
                }
            }
        }),
        kind => element(out, tag, |o| scalar(kind, v, o)),
    }
}

fn scalar(kind: u8, v: &Value, out: &mut String) {
    match (kind, v) {
        (K_TS, Value::Number(n)) => {
            #[allow(clippy::cast_possible_truncation)]
            let secs = n.as_f64().unwrap_or(0.0).floor() as i64;
            out.push_str(&format_iso8601(secs));
        }
        (K_BLOB, Value::String(s)) | (_, Value::String(s)) => escape(out, s),
        (_, Value::Number(n)) => out.push_str(&n.to_string()),
        (_, Value::Bool(b)) => out.push_str(if *b { "true" } else { "false" }),
        _ => {}
    }
}

/// The whole success answer.
pub(super) fn response(
    t: &Tables,
    namespace: Option<&str>,
    operation: &str,
    output: Option<(u32, &Value)>,
    request_id: &str,
) -> String {
    let mut out = String::from("<?xml version=\"1.0\"?>\n");
    out.push('<');
    out.push_str(operation);
    out.push_str("Response");
    if let Some(ns) = namespace {
        out.push_str(" xmlns=\"");
        escape(&mut out, ns);
        out.push('"');
    }
    out.push('>');
    if let Some((shape, v)) = output {
        element(&mut out, &format!("{operation}Result"), |o| {
            members(t, shape, v, o, 0)
        });
    }
    element(&mut out, "ResponseMetadata", |o| {
        element(o, "RequestId", |r| escape(r, request_id))
    });
    out.push_str("</");
    out.push_str(operation);
    out.push_str("Response>");
    out
}

/// The error answer.
pub(super) fn error(
    namespace: Option<&str>,
    who: &str,
    code: &str,
    message: &str,
    request_id: &str,
) -> String {
    let mut out = String::from("<?xml version=\"1.0\"?>\n<ErrorResponse");
    if let Some(ns) = namespace {
        out.push_str(" xmlns=\"");
        escape(&mut out, ns);
        out.push('"');
    }
    out.push('>');
    element(&mut out, "Error", |o| {
        element(o, "Type", |x| x.push_str(who));
        element(o, "Code", |x| escape(x, code));
        element(o, "Message", |x| escape(x, message));
    });
    element(&mut out, "RequestId", |r| escape(r, request_id));
    out.push_str("</ErrorResponse>");
    out
}
