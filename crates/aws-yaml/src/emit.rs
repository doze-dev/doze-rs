//! Writing JSON as block YAML: what `GetDocument` answers when a document stored as JSON is asked for as YAML.
//!
//! Every string is written double-quoted with JSON's escapes (YAML's double-quoted style reads them), so no string can be read
//! back as a number, a boolean or null; mappings and sequences are block style, two spaces deep.

use serde_json::Value;

/// `value` as a YAML document, ending with a newline.
#[must_use]
pub fn emit(value: &Value) -> String {
    let mut out = String::new();
    node(value, 0, &mut out);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn scalar(value: &Value) -> Option<String> {
    Some(match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => quoted(s),
        Value::Array(a) if a.is_empty() => "[]".to_owned(),
        Value::Object(o) if o.is_empty() => "{}".to_owned(),
        Value::Array(_) | Value::Object(_) => return None,
    })
}

fn quoted(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned())
}

/// Writes `value` with each of its lines indented by `indent` spaces.
fn node(value: &Value, indent: usize, out: &mut String) {
    if let Some(s) = scalar(value) {
        out.push_str(&" ".repeat(indent));
        out.push_str(&s);
        out.push('\n');
        return;
    }
    let pad = " ".repeat(indent);
    match value {
        Value::Object(o) => {
            for (k, v) in o {
                out.push_str(&pad);
                out.push_str(&quoted(k));
                out.push(':');
                match scalar(v) {
                    Some(s) => {
                        out.push(' ');
                        out.push_str(&s);
                        out.push('\n');
                    }
                    None => {
                        out.push('\n');
                        node(v, indent + 2, out);
                    }
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                let mut inner = String::new();
                node(v, indent + 2, &mut inner);
                out.push_str(&pad);
                out.push_str("- ");
                out.push_str(inner.get(indent + 2..).unwrap_or_default());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use serde_json::json;

    use super::emit;
    use crate::parse;

    #[test]
    fn what_is_written_reads_back_the_same() {
        let doc = json!({
            "schemaVersion": "2.2",
            "description": "say: \"hi\"\nthen go",
            "parameters": {"Message": {"type": "String", "default": "true"}, "Empty": {}},
            "mainSteps": [
                {"action": "aws:runShellScript", "name": "run", "inputs": {"runCommand": ["echo 1", "echo 2"]}},
                [1, 2.5, null, false, []],
                "plain"
            ],
            "count": 3
        });
        let text = emit(&doc);
        assert_eq!(parse(&text).unwrap(), doc, "{text}");
        assert!(text.lines().any(|l| l == "\"schemaVersion\": \"2.2\""), "{text}");
    }

    #[test]
    fn a_scalar_document_is_one_line() {
        assert_eq!(emit(&json!("x")), "\"x\"\n");
        assert_eq!(emit(&json!({})), "{}\n");
    }
}
