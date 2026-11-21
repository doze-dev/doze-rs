//! A JSON document written for a person: objects one member to a line, indented by two, and a short list of plain values
//! (`"source": ["larder.orders"]`) on the one line it fits, where `serde_json`'s pretty form gives each value a line of its own.

use serde_json::Value;

/// The widest a list of plain values may be and stay on one line.
const INLINE: usize = 72;

/// `value` written out.
#[must_use]
pub(super) fn pretty(value: &Value) -> String {
    let mut out = String::new();
    write(value, 0, &mut out);
    out
}

fn write(value: &Value, depth: usize, out: &mut String) {
    match value {
        Value::Object(m) if !m.is_empty() => {
            out.push_str("{\n");
            for (i, (k, v)) in m.iter().enumerate() {
                indent(depth + 1, out);
                out.push_str(&Value::String(k.clone()).to_string());
                out.push_str(": ");
                write(v, depth + 1, out);
                out.push_str(if i + 1 < m.len() { ",\n" } else { "\n" });
            }
            indent(depth, out);
            out.push('}');
        }
        Value::Array(a) if !a.is_empty() => {
            let plain = a
                .iter()
                .all(|v| !matches!(v, Value::Object(_) | Value::Array(_)));
            let line = format!(
                "[{}]",
                a.iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if plain && line.chars().count() + depth * 2 <= INLINE {
                out.push_str(&line);
                return;
            }
            out.push_str("[\n");
            for (i, v) in a.iter().enumerate() {
                indent(depth + 1, out);
                write(v, depth + 1, out);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            indent(depth, out);
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

fn indent(depth: usize, out: &mut String) {
    out.push_str(&"  ".repeat(depth));
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::pretty;

    #[test]
    fn a_short_list_of_plain_values_stays_on_its_line() {
        let v =
            json!({"source": ["larder.orders"], "detail-type": ["OrderPlaced", "OrderChanged"]});
        assert_eq!(
            pretty(&v),
            "{\n  \"source\": [\"larder.orders\"],\n  \"detail-type\": [\"OrderPlaced\", \"OrderChanged\"]\n}"
        );
    }

    #[test]
    fn nesting_a_long_list_and_empty_values() {
        let long: Vec<String> = (0..12).map(|i| format!("value-number-{i}")).collect();
        let v = json!({"detail": {"a": {"b": long}}, "none": [], "empty": {}, "n": 1, "t": true, "z": null});
        let text = pretty(&v);
        assert!(text.contains("\"b\": [\n"), "{text}");
        assert!(text.contains("\"none\": []") && text.contains("\"empty\": {}"));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap(), v);
    }

    #[test]
    fn a_list_of_objects_is_expanded() {
        let text = pretty(&json!({"Statement": [{"Effect": "Allow"}]}));
        assert_eq!(
            text,
            "{\n  \"Statement\": [\n    {\n      \"Effect\": \"Allow\"\n    }\n  ]\n}"
        );
    }
}
