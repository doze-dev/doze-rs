//! A schema's text laid out for reading: JSON with a field or a property a line, Protobuf as Confluent prints it.

use kafka_registry::format::SchemaType;
use kafka_registry::json::Json;

/// A schema's text laid out for reading.
#[must_use]
pub fn laid_out(kind: SchemaType, text: &str) -> String {
    match kind {
        SchemaType::Protobuf => text.to_owned(),
        _ => Json::parse(text).map_or_else(
            |_| text.to_owned(),
            |j| {
                let mut out = String::new();
                fold(&j, 0, true, &mut out);
                out
            },
        ),
    }
}

/// The members always opened up, one a line: a record's fields, an object's properties, the definitions.
const OPEN: [&str; 5] = ["fields", "properties", "definitions", "$defs", "oneOf"];

/// How wide a part of a schema may be to stay on one line.
const ONE_LINE: usize = 76;

/// JSON written on one line with a space after each `:` and `,`.
fn spaced(j: &Json, out: &mut String) {
    match j {
        Json::Object(m) => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                kafka_registry::json::write_str(out, k);
                out.push_str(": ");
                spaced(v, out);
            }
            out.push('}');
        }
        Json::Array(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                spaced(v, out);
            }
            out.push(']');
        }
        other => other.write(out),
    }
}

/// A schema laid out for reading: a part that fits on a line below the top stays on one (a field, a property), the rest
/// is opened up two spaces a level. A changed field is then one line of a diff.
fn fold(j: &Json, level: usize, open: bool, out: &mut String) {
    let mut one = String::new();
    spaced(j, &mut one);
    let fits = !open && one.chars().count() + level * 2 <= ONE_LINE;
    let pad = |out: &mut String, n: usize| {
        out.push('\n');
        out.push_str(&"  ".repeat(n));
    };
    match j {
        Json::Object(m) if !fits && !m.is_empty() => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                out.push_str(if i > 0 { "," } else { "" });
                pad(out, level + 1);
                kafka_registry::json::write_str(out, k);
                out.push_str(": ");
                fold(v, level + 1, OPEN.contains(&k.as_str()), out);
            }
            pad(out, level);
            out.push('}');
        }
        Json::Array(items) if !fits && !items.is_empty() => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                out.push_str(if i > 0 { "," } else { "" });
                pad(out, level + 1);
                fold(v, level + 1, false, out);
            }
            pad(out, level);
            out.push(']');
        }
        _ => out.push_str(&one),
    }
}
