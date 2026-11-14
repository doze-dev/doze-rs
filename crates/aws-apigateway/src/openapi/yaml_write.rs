//! JSON written as YAML, for an export asked for as YAML: block mappings and sequences, every scalar written as JSON writes it (a
//! quoted string is valid YAML), so what is written reads back through [`super::yaml`] as the same value.

use serde_json::Value;

/// A JSON value written as YAML (for exports asked for as YAML).
pub(crate) fn write(v: &Value) -> String {
    let mut out = String::new();
    emit(v, 0, &mut out);
    out
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => Value::String(s.clone()).to_string(),
        other => other.to_string(),
    }
}

fn emit(v: &Value, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match v {
        Value::Object(o) if !o.is_empty() => {
            for (k, x) in o {
                let key = Value::String(k.clone()).to_string();
                match x {
                    Value::Object(m) if !m.is_empty() => {
                        out.push_str(&format!("{pad}{key}:\n"));
                        emit(x, indent + 2, out);
                    }
                    Value::Array(a) if !a.is_empty() => {
                        out.push_str(&format!("{pad}{key}:\n"));
                        emit(x, indent, out);
                    }
                    _ => out.push_str(&format!("{pad}{key}: {}\n", scalar(x))),
                }
            }
        }
        Value::Array(a) if !a.is_empty() => {
            for x in a {
                match x {
                    Value::Object(m) if !m.is_empty() => {
                        out.push_str(&format!("{pad}-\n"));
                        emit(x, indent + 2, out);
                    }
                    _ => out.push_str(&format!("{pad}- {}\n", scalar(x))),
                }
            }
        }
        other => out.push_str(&format!("{pad}{}\n", scalar(other))),
    }
}
