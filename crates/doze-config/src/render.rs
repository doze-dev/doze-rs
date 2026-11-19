//! A value as text (Go's `renderValue`): a string as it is, anything else as HCL (hclwrite's
//! `TokensForValue`, formatted).

use doze_hcl::Value;

/// "" for null or not yet known; the string itself; else the value written as HCL.
pub(crate) fn render_value(v: &Value) -> String {
    if v.is_null() || !v.is_wholly_known() {
        return String::new();
    }
    if let Value::String(s) = v {
        return s.clone();
    }
    let raw = tokens(v);
    String::from_utf8_lossy(&doze_hcl::fmt::format(raw.as_bytes())).into_owned()
}

fn tokens(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.text(),
        Value::String(s) => quoted(s),
        Value::List(_, es) | Value::Set(_, es) | Value::Tuple(es) => {
            format!("[{}]", es.iter().map(tokens).collect::<Vec<_>>().join(", "))
        }
        Value::Map(_, m) | Value::Object(m) => {
            let mut out = String::from("{\n");
            for (k, e) in m {
                let key = if is_ident(k) { k.clone() } else { quoted(k) };
                out.push_str(&format!("{key} = {}\n", tokens(e)));
            }
            out.push('}');
            out
        }
        Value::Null(_) => "null".into(),
        Value::Unknown(..) => String::new(),
    }
}

/// hclwrite's `escapeQuotedStringLit`, quoted.
fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' | '%' if chars.peek() == Some(&'{') => {
                out.push(c);
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A valid HCL identifier (a key written bare).
fn is_ident(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && cs.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}
