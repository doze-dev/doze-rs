//! The small languages of the plane: query strings, the JSONPath of `$input.path` and `$request.body`, and parameter mappings
//! (`method.request.header.x`, `'literal'`, `stageVariables.x` on REST; `$request.header.x`, `$context.x`, `$stageVariables.x` on HTTP
//! APIs).

use serde_json::{Map, Value};

use super::Incoming;

/// The decoded pairs of a query string, in order (`+` is a space).
pub(crate) fn query_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            let dec = |s: &str| {
                let s = s.replace('+', " ");
                aws_core::uri::decode_str(&s).map_or(s.clone(), |c| c.into_owned())
            };
            (dec(k), dec(v))
        })
        .collect()
}

/// The value at a JSONPath (`$`, `$.a.b`, `$['a']`, `$.a[0]`, `$.a[*].b`) in `doc`.
pub(crate) fn jsonpath(doc: &Value, path: &str) -> Option<Value> {
    let rest = path.trim().strip_prefix('$')?;
    let mut cur = vec![doc.clone()];
    let mut spread = false;
    let mut s = rest;
    while !s.is_empty() {
        let (step, next) = if let Some(after) = s.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            (Step::Key(after.get(..end)?.to_owned()), after.get(end..)?)
        } else if let Some(after) = s.strip_prefix('[') {
            let end = after.find(']')?;
            let inner = after.get(..end)?.trim();
            let step = if inner == "*" {
                Step::All
            } else if let Ok(i) = inner.parse::<usize>() {
                Step::Index(i)
            } else {
                Step::Key(inner.trim_matches(['\'', '"']).to_owned())
            };
            (step, after.get(end + 1..)?)
        } else {
            return None;
        };
        if matches!(step, Step::All) {
            spread = true;
        }
        cur = cur.into_iter().flat_map(|v| step.apply(v)).collect();
        s = next;
    }
    if spread {
        Some(Value::Array(cur))
    } else {
        cur.into_iter().next()
    }
}

enum Step {
    Key(String),
    Index(usize),
    All,
}

impl Step {
    fn apply(&self, v: Value) -> Vec<Value> {
        match (self, v) {
            (Self::Key(k), Value::Object(mut o)) => o.remove(k).into_iter().collect(),
            (Self::Index(i), Value::Array(mut a)) if *i < a.len() => vec![a.swap_remove(*i)],
            (Self::All, Value::Array(a)) => a,
            (Self::All, Value::Object(o)) => o.into_values().collect(),
            _ => Vec::new(),
        }
    }
}

/// `{path, querystring, header}` of a request, as `$input.params()` shows it.
pub(crate) fn params_of(req: &Incoming, path: &[(String, String)]) -> Value {
    let to_map = |pairs: &[(String, String)]| -> Map<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect()
    };
    serde_json::json!({
        "path": to_map(path),
        "querystring": to_map(&req.query),
        "header": header_map(req),
    })
}

/// The headers as a map with each name's last value, names as sent (lower case here) — what a proxy event's `headers` is.
pub(crate) fn header_map(req: &Incoming) -> Map<String, Value> {
    req.headers
        .iter()
        .map(|(k, v)| (canonical(k), Value::String(v.clone())))
        .collect()
}

/// A header name in the capitalisation clients usually send (`content-type` as `Content-Type`), as API Gateway passes on what it got.
pub(crate) fn canonical(name: &str) -> String {
    name.split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// The value a REST parameter mapping source names: `method.request.{path,querystring,header}.x`, `method.request.body`,
/// `method.request.body.<jsonpath>`, `stageVariables.x`, `context.x`, or a quoted literal.
pub(crate) fn rest_source(
    src: &str,
    req: &Incoming,
    path: &[(String, String)],
    ctx: &Value,
    vars: &Value,
) -> Option<String> {
    let src = src.trim();
    if let Some(lit) = src.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Some(lit.to_owned());
    }
    if let Some(name) = src.strip_prefix("method.request.path.") {
        return path.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    }
    if let Some(name) = src.strip_prefix("method.request.querystring.") {
        return req.query_value(name).map(str::to_owned);
    }
    if let Some(name) = src.strip_prefix("method.request.multivaluequerystring.") {
        let all: Vec<&str> = req
            .query
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .collect();
        return (!all.is_empty()).then(|| all.join(","));
    }
    if let Some(name) = src
        .strip_prefix("method.request.header.")
        .or_else(|| src.strip_prefix("method.request.multivalueheader."))
    {
        return req.header(&name.to_ascii_lowercase()).map(str::to_owned);
    }
    if src == "method.request.body" {
        return Some(String::from_utf8_lossy(&req.body).into_owned());
    }
    if let Some(p) = src.strip_prefix("method.request.body.") {
        let doc: Value = serde_json::from_slice(&req.body).ok()?;
        return jsonpath(&doc, &format!("$.{p}")).map(|v| plain(&v));
    }
    if let Some(name) = src.strip_prefix("stageVariables.") {
        return vars.get(name).map(plain);
    }
    let name = src.strip_prefix("context.")?;
    let mut cur = ctx;
    for seg in name.split('.') {
        cur = cur.get(seg)?;
    }
    Some(plain(cur))
}

/// The value an HTTP API mapping names: `$request.header.x`, `$request.querystring.x`, `$request.path.x`, `$request.path`,
/// `$request.body.<jsonpath>`, `$context.x`, `$stageVariables.x`, or text with those inside it.
pub(crate) fn v2_source(
    src: &str,
    req: &Incoming,
    path: &[(String, String)],
    raw_path: &str,
    ctx: &Value,
    vars: &Value,
) -> String {
    let mut out = String::new();
    let mut rest = src;
    while let Some(i) = rest.find('$') {
        out.push_str(rest.get(..i).unwrap_or_default());
        let after = rest.get(i..).unwrap_or_default();
        let end = after
            .char_indices()
            .skip(1)
            .find(|(_, c)| {
                !(c.is_ascii_alphanumeric()
                    || matches!(c, '.' | '_' | '-' | '[' | ']' | '*' | '\''))
            })
            .map_or(after.len(), |(j, _)| j);
        let expr = after.get(..end).unwrap_or_default();
        out.push_str(&v2_one(expr, req, path, raw_path, ctx, vars).unwrap_or_default());
        rest = after.get(end..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

fn v2_one(
    e: &str,
    req: &Incoming,
    path: &[(String, String)],
    raw_path: &str,
    ctx: &Value,
    vars: &Value,
) -> Option<String> {
    if let Some(n) = e.strip_prefix("$request.header.") {
        return req.header(&n.to_ascii_lowercase()).map(str::to_owned);
    }
    if let Some(n) = e.strip_prefix("$request.querystring.") {
        return req.query_value(n).map(str::to_owned);
    }
    if let Some(n) = e.strip_prefix("$request.path.") {
        return path.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    }
    if e == "$request.path" {
        return Some(raw_path.to_owned());
    }
    if e == "$request.body" {
        return Some(String::from_utf8_lossy(&req.body).into_owned());
    }
    if let Some(p) = e.strip_prefix("$request.body") {
        let doc: Value = serde_json::from_slice(&req.body).ok()?;
        return jsonpath(&doc, &format!("${p}")).map(|v| plain(&v));
    }
    if let Some(n) = e.strip_prefix("$stageVariables.") {
        return vars.get(n).map(plain);
    }
    let name = e.strip_prefix("$context.")?;
    let mut cur = ctx;
    for seg in name.split('.') {
        cur = cur.get(seg)?;
    }
    Some(plain(cur))
}

/// A JSON value as the text a mapping puts in a header or a parameter (a string without its quotes).
pub(crate) fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn json_paths_reach_into_objects_and_lists() {
        let doc = json!({"a": {"b": [10, {"c": "x"}]}, "list": [{"v": 1}, {"v": 2}]});
        assert_eq!(jsonpath(&doc, "$"), Some(doc.clone()));
        assert_eq!(jsonpath(&doc, "$.a.b[1].c"), Some(json!("x")));
        assert_eq!(jsonpath(&doc, "$['a']['b'][0]"), Some(json!(10)));
        assert_eq!(jsonpath(&doc, "$.list[*].v"), Some(json!([1, 2])));
        assert_eq!(jsonpath(&doc, "$.nope"), None);
        assert_eq!(jsonpath(&doc, "a"), None);
    }

    #[test]
    fn queries_are_decoded() {
        assert_eq!(
            query_pairs("a=1&b=x+y&c=%2F&d"),
            [("a", "1"), ("b", "x y"), ("c", "/"), ("d", "")]
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
    }
}
