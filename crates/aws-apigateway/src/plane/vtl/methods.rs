//! What templates call: `$input`, `$util`, and the Java methods of strings, lists and maps that templates use. A method that is not
//! here is refused by name.

use serde_json::{Map, Value};

use super::Input;
use super::eval::{num, text_of};
use super::parse::Error;
use crate::plane::mapping::jsonpath;

type R<T> = Result<T, Error>;

fn not_built<T>(on: &str, name: &str) -> R<T> {
    Err(Error(format!(
        "{on}.{name}() is not built in doze-aws's mapping templates"
    )))
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

fn arg_text(args: &[Value], i: usize) -> String {
    text_of(&arg(args, i))
}

/// `$input.<name>(...)` and `$util.<name>(...)`.
pub(super) fn builtin(root: &str, name: &str, args: &[Value], input: &Input) -> R<Value> {
    if root == "util" {
        return util(name, args);
    }
    let body_json = || serde_json::from_str::<Value>(&input.body).unwrap_or(Value::Null);
    Ok(match name {
        "body" => Value::String(input.body.clone()),
        "json" => {
            let at = jsonpath(&body_json(), &arg_text(args, 0)).unwrap_or(Value::Null);
            Value::String(at.to_string())
        }
        "path" => jsonpath(&body_json(), &arg_text(args, 0)).unwrap_or(Value::Null),
        "params" if args.is_empty() => input.params.clone(),
        "params" => {
            let want = arg_text(args, 0);
            ["path", "querystring", "header"]
                .iter()
                .find_map(|kind| {
                    input
                        .params
                        .get(kind)
                        .and_then(Value::as_object)
                        .and_then(|m| {
                            m.iter()
                                .find(|(k, _)| {
                                    if *kind == "header" {
                                        k.eq_ignore_ascii_case(&want)
                                    } else {
                                        **k == want
                                    }
                                })
                                .map(|(_, v)| v.clone())
                        })
                })
                .unwrap_or(Value::Null)
        }
        _ => return not_built("$input", name),
    })
}

fn util(name: &str, args: &[Value]) -> R<Value> {
    let s = arg_text(args, 0);
    Ok(match name {
        "escapeJavaScript" => Value::String(escape_js(&s)),
        "parseJson" => serde_json::from_str(&s).unwrap_or(Value::Null),
        "urlEncode" => Value::String(aws_core::uri::encode(s.as_bytes(), false)),
        "urlDecode" => Value::String(
            aws_core::uri::decode_str(&s.replace('+', " "))
                .map(|c| c.into_owned())
                .unwrap_or(s),
        ),
        "base64Encode" => Value::String(aws_core::base64::encode(s.as_bytes())),
        "base64Decode" => Value::String(
            aws_core::base64::decode(&s)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default(),
        ),
        _ => return not_built("$util", name),
    })
}

/// Java's `StringEscapeUtils.escapeJavaScript`, which `$util.escapeJavaScript` is: quotes (both), backslash, slash and the control
/// characters escaped.
fn escape_js(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

/// `$x.name`: a map's member.
pub(super) fn property(v: &Value, name: &str) -> Value {
    match v {
        Value::Object(o) => o.get(name).cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// `$x.name(...)` on a value.
pub(super) fn call(v: &Value, name: &str, args: &[Value]) -> R<Value> {
    if let Some(out) = common(v, name, args) {
        return Ok(out);
    }
    match v {
        Value::String(s) => string(s, name, args),
        Value::Array(a) => list(a, name, args),
        Value::Object(o) => map(o, name, args),
        _ => not_built("a value", name),
    }
}

fn common(v: &Value, name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "toString" => Value::String(text_of(v)),
        "equals" => Value::Bool(*v == arg(args, 0) || text_of(v) == arg_text(args, 0)),
        "isEmpty" => Value::Bool(match v {
            Value::String(s) => s.is_empty(),
            Value::Array(a) => a.is_empty(),
            Value::Object(o) => o.is_empty(),
            _ => v.is_null(),
        }),
        "size" | "length" => Value::from(match v {
            Value::String(s) => s.chars().count(),
            Value::Array(a) => a.len(),
            Value::Object(o) => o.len(),
            _ => 0,
        }),
        _ => return None,
    })
}

fn regex(pattern: &str) -> R<regex_lite::Regex> {
    regex_lite::Regex::new(pattern).map_err(|e| Error(format!("a pattern is not valid: {e}")))
}

fn string(s: &str, name: &str, args: &[Value]) -> R<Value> {
    let a = arg_text(args, 0);
    Ok(match name {
        "contains" => Value::Bool(s.contains(&a)),
        "startsWith" => Value::Bool(s.starts_with(&a)),
        "endsWith" => Value::Bool(s.ends_with(&a)),
        "equalsIgnoreCase" => Value::Bool(s.eq_ignore_ascii_case(&a)),
        "toLowerCase" => Value::String(s.to_lowercase()),
        "toUpperCase" => Value::String(s.to_uppercase()),
        "trim" => Value::String(s.trim().to_owned()),
        "replace" => Value::String(s.replace(&a, &arg_text(args, 1))),
        "replaceAll" => Value::String(
            regex(&a)?
                .replace_all(s, arg_text(args, 1).as_str())
                .into_owned(),
        ),
        "matches" => Value::Bool(regex(&format!("^(?:{a})$"))?.is_match(s)),
        "split" => Value::Array(
            regex(&a)?
                .split(s)
                .map(|p| Value::String(p.to_owned()))
                .collect(),
        ),
        "indexOf" => Value::from(
            s.find(&a)
                .map_or(-1, |i| i64::try_from(s[..i].chars().count()).unwrap_or(-1)),
        ),
        "charAt" => Value::String(
            s.chars()
                .nth(num(&arg(args, 0)) as usize)
                .map(String::from)
                .unwrap_or_default(),
        ),
        "substring" => {
            let chars: Vec<char> = s.chars().collect();
            let from = (num(&arg(args, 0)) as usize).min(chars.len());
            let to = if args.len() > 1 {
                (num(&arg(args, 1)) as usize).min(chars.len())
            } else {
                chars.len()
            };
            Value::String(
                chars
                    .get(from..to.max(from))
                    .unwrap_or_default()
                    .iter()
                    .collect(),
            )
        }
        _ => return not_built("a string", name),
    })
}

fn list(a: &[Value], name: &str, args: &[Value]) -> R<Value> {
    Ok(match name {
        "get" => a
            .get(num(&arg(args, 0)) as usize)
            .cloned()
            .unwrap_or(Value::Null),
        "contains" => Value::Bool(a.contains(&arg(args, 0))),
        "indexOf" => Value::from(
            a.iter()
                .position(|x| *x == arg(args, 0))
                .map_or(-1, |i| i as i64),
        ),
        _ => return not_built("a list", name),
    })
}

fn map(o: &Map<String, Value>, name: &str, args: &[Value]) -> R<Value> {
    let k = arg_text(args, 0);
    Ok(match name {
        "get" => o.get(&k).cloned().unwrap_or(Value::Null),
        "containsKey" => Value::Bool(o.contains_key(&k)),
        "keySet" => Value::Array(o.keys().map(|k| Value::String(k.clone())).collect()),
        "values" => Value::Array(o.values().cloned().collect()),
        "entrySet" => Value::Array(
            o.iter()
                .map(|(k, v)| serde_json::json!({"key": k, "value": v}))
                .collect(),
        ),
        _ => return not_built("a map", name),
    })
}

/// A call that changes the value it is made on (`add`, `put`, `remove`, `addAll`, `putAll`), and what it answers.
pub(super) fn mutate(holder: &mut Value, name: &str, args: &[Value]) -> R<Value> {
    match (holder, name) {
        (Value::Array(a), "add") => {
            a.push(arg(args, 0));
            Ok(Value::Bool(true))
        }
        (Value::Array(a), "addAll") => {
            if let Value::Array(more) = arg(args, 0) {
                a.extend(more);
            }
            Ok(Value::Bool(true))
        }
        (Value::Array(a), "remove") => {
            let i = num(&arg(args, 0)) as usize;
            Ok(if i < a.len() {
                a.remove(i)
            } else {
                Value::Null
            })
        }
        (Value::Object(o), "put") => Ok(o
            .insert(arg_text(args, 0), arg(args, 1))
            .unwrap_or(Value::Null)),
        (Value::Object(o), "putAll") => {
            if let Value::Object(more) = arg(args, 0) {
                o.extend(more);
            }
            Ok(Value::Null)
        }
        (Value::Object(o), "remove") => Ok(o.remove(&arg_text(args, 0)).unwrap_or(Value::Null)),
        (_, other) => not_built("this value", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_java_script_escapes_as_java_does() {
        assert_eq!(escape_js("a\"b'c/d\\e\nf"), "a\\\"b\\'c\\/d\\\\e\\nf");
    }
}
