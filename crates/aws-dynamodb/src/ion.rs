//! Amazon Ion text, as far as DynamoDB exports write it and imports read it (the developer guide's mapping): a string is an Ion
//! string, a number an Ion decimal (`103.`, `6d2`), binary a blob (`{{...}}`), a boolean and null as themselves, a list an Ion list,
//! a map a struct, and a set a list annotated `$dynamodb_SS`, `$dynamodb_NS` or `$dynamodb_BS`. The reader takes the rest of the
//! text form a hand-written file may use (integers, floats, symbols, quoted field names, comments, typed nulls) and refuses what has
//! no DynamoDB meaning (timestamps, clobs, s-expressions). It is bounded: nesting deeper than an item may be is refused.

use aws_expr::{Number, Value};

/// The version marker each line of an export starts with.
pub(crate) const MARKER: &str = "$ion_1_0";

/// One line of an export: `$ion_1_0 {<wrapper>:<value>}` (`Item` for a full export, `Record` for an incremental one).
pub(crate) fn line(wrapper: &str, value: &Value) -> String {
    let mut out = String::from(MARKER);
    out.push_str(" {");
    out.push_str(wrapper);
    out.push(':');
    write(value, &mut out);
    out.push('}');
    out
}

fn field_name(k: &str, out: &mut String) {
    let ident = k
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && !matches!(k, "null" | "true" | "false" | "nan");
    if ident {
        out.push_str(k);
    } else {
        string(k, out);
    }
}

fn string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\x{:02x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A number as an Ion decimal: an exponent is written with `d`, and a whole number ends in `.`.
fn decimal(n: &Number, out: &mut String) {
    let text = n.to_string();
    if let Some((m, e)) = text.split_once('E') {
        out.push_str(m);
        out.push('d');
        out.push_str(e.trim_start_matches('+'));
    } else if text.contains('.') {
        out.push_str(&text);
    } else {
        out.push_str(&text);
        out.push('.');
    }
}

fn blob(b: &[u8], out: &mut String) {
    out.push_str("{{");
    out.push_str(&aws_core::base64::encode(b));
    out.push_str("}}");
}

fn list<T>(
    items: impl IntoIterator<Item = T>,
    out: &mut String,
    mut each: impl FnMut(T, &mut String),
) {
    out.push('[');
    for (i, v) in items.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        each(v, out);
    }
    out.push(']');
}

/// A value in Ion text.
pub(crate) fn write(v: &Value, out: &mut String) {
    match v {
        Value::S(s) => string(s, out),
        Value::N(n) => decimal(n, out),
        Value::B(b) => blob(b, out),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
        Value::Ss(s) => {
            out.push_str("$dynamodb_SS::");
            list(s, out, |s, o| string(s, o));
        }
        Value::Ns(s) => {
            out.push_str("$dynamodb_NS::");
            list(s, out, decimal);
        }
        Value::Bs(s) => {
            out.push_str("$dynamodb_BS::");
            list(s, out, |b, o| blob(b, o));
        }
        Value::L(l) => list(l, out, write),
        Value::M(m) => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                field_name(k, out);
                out.push(':');
                write(v, out);
            }
            out.push('}');
        }
    }
}

mod read;

pub(crate) use read::{Ion, read_all, to_value};

#[cfg(test)]
mod tests;
