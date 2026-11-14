//! JSON to Avro binary, as the Go edition's console sent it: a field left out takes its default, a union's value may be
//! tagged with its branch (`{"double": 0}`, what a decoded value shows) or is tried branch by branch, bytes and fixed are
//! one character a byte (or base64).

use super::codec::branch_name;
use super::schema::{NamedKind, Schema, Ty};
use crate::codec::text_bytes;
use crate::json::Json;

type Res<T> = Result<T, String>;

/// Writes `value` as `schema`'s root type.
pub(crate) fn encode(schema: &Schema, value: &Json) -> Res<Vec<u8>> {
    let root = schema.root.as_ref().ok_or("the schema has no type")?;
    let mut out = Vec::new();
    write(schema, root, value, &mut out, 0)?;
    Ok(out)
}

fn put_long(out: &mut Vec<u8>, n: i64) {
    let mut v = ((n << 1) ^ (n >> 63)).cast_unsigned();
    while v >= 0x80 {
        out.push(u8::try_from(v & 0x7f).unwrap_or(0) | 0x80);
        v >>= 7;
    }
    out.push(u8::try_from(v).unwrap_or(0));
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_long(out, i64::try_from(b.len()).unwrap_or(i64::MAX));
    out.extend_from_slice(b);
}

fn number(v: &Json) -> Res<&str> {
    match v {
        Json::Raw(t) if !matches!(t.as_str(), "true" | "false" | "null") => Ok(t),
        _ => Err(format!("expected a number, not {}", v.to_text())),
    }
}

fn whole(v: &Json) -> Res<i64> {
    let t = number(v)?;
    t.parse::<i64>()
        .map_err(|_| format!("expected a whole number, not {t}"))
}

fn real(v: &Json) -> Res<f64> {
    // JSON has no number for these: they are written as words (as a decoded value shows them).
    match v.as_str() {
        Some("NaN") => return Ok(f64::NAN),
        Some("Infinity") => return Ok(f64::INFINITY),
        Some("-Infinity") => return Ok(f64::NEG_INFINITY),
        _ => {}
    }
    let t = number(v)?;
    t.parse::<f64>().map_err(|_| format!("{t} is not a number"))
}

fn text(v: &Json) -> Res<&str> {
    v.as_str()
        .ok_or_else(|| format!("expected a string, not {}", v.to_text()))
}

fn write(schema: &Schema, t: &Ty, v: &Json, out: &mut Vec<u8>, depth: usize) -> Res<()> {
    if depth > crate::codec::MAX_DEPTH {
        return Err("the value nests too deep".into());
    }
    match t {
        Ty::Null => match v {
            Json::Raw(n) if n == "null" => Ok(()),
            _ => Err(format!("expected null, not {}", v.to_text())),
        },
        Ty::Boolean => match v {
            Json::Raw(b) if b == "true" || b == "false" => {
                out.push(u8::from(b == "true"));
                Ok(())
            }
            _ => Err(format!("expected true or false, not {}", v.to_text())),
        },
        Ty::Int => {
            let n = whole(v)?;
            let n = i32::try_from(n).map_err(|_| format!("{n} is too large for an int"))?;
            put_long(out, i64::from(n));
            Ok(())
        }
        Ty::Long => {
            put_long(out, whole(v)?);
            Ok(())
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a float is the double rounded"
        )]
        Ty::Float => {
            out.extend((real(v)? as f32).to_le_bytes());
            Ok(())
        }
        Ty::Double => {
            out.extend(real(v)?.to_le_bytes());
            Ok(())
        }
        Ty::Bytes => {
            put_bytes(out, &text_bytes(text(v)?)?);
            Ok(())
        }
        Ty::String => {
            put_bytes(out, text(v)?.as_bytes());
            Ok(())
        }
        Ty::Array(items) => write_array(schema, items, v, out, depth),
        Ty::Map(values) => write_map(schema, values, v, out, depth),
        Ty::Union(branches) => union(schema, branches, v, out, depth),
        Ty::Named(i) => named_out(schema, *i, v, out, depth),
    }
}

fn union(schema: &Schema, branches: &[Ty], v: &Json, out: &mut Vec<u8>, depth: usize) -> Res<()> {
    let index = |i: usize| i64::try_from(i).unwrap_or(i64::MAX);
    if matches!(v, Json::Raw(n) if n == "null") {
        let i = branches
            .iter()
            .position(|b| *b == Ty::Null)
            .ok_or("null is not one of the union's branches")?;
        put_long(out, index(i));
        return Ok(());
    }
    // The tagged form, {"double": 0}: what a decoded value shows, so a record read here can be sent as it stands.
    if let Json::Object(m) = v
        && let [(tag, inner)] = m.as_slice()
    {
        for (i, b) in branches.iter().enumerate() {
            if *b != Ty::Null && branch_name(schema, b) == *tag {
                let mut body = Vec::new();
                if write(schema, b, inner, &mut body, depth + 1).is_ok() {
                    put_long(out, index(i));
                    out.extend(body);
                    return Ok(());
                }
            }
        }
    }
    let mut why = Vec::new();
    for (i, b) in branches.iter().enumerate().filter(|(_, b)| **b != Ty::Null) {
        let mut body = Vec::new();
        match write(schema, b, v, &mut body, depth + 1) {
            Ok(()) => {
                put_long(out, index(i));
                out.extend(body);
                return Ok(());
            }
            Err(e) => why.push(e),
        }
    }
    Err(format!("no branch of the union fits: {}", why.join("; ")))
}

fn named_out(schema: &Schema, i: usize, v: &Json, out: &mut Vec<u8>, depth: usize) -> Res<()> {
    let n = schema.named(i).ok_or("a type the schema does not define")?;
    match &n.kind {
        NamedKind::Record(fields) => {
            if !matches!(v, Json::Object(_)) {
                return Err(format!(
                    "record {} needs a JSON object, not {}",
                    n.full,
                    v.to_text()
                ));
            }
            for f in fields {
                let value = match (v.get(&f.name), &f.default) {
                    (Some(x), _) => x,
                    (None, Some(d)) => d,
                    (None, None) => {
                        return Err(format!("field {:?} is missing and has no default", f.name));
                    }
                };
                write(schema, &f.ty, value, out, depth + 1)
                    .map_err(|e| format!("{}: {e}", f.name))?;
            }
            Ok(())
        }
        NamedKind::Enum { symbols, .. } => {
            let s = text(v)?;
            let i = symbols.iter().position(|x| x == s).ok_or_else(|| {
                format!(
                    "{s:?} is not a symbol of enum {} ({})",
                    n.full,
                    symbols.join(", ")
                )
            })?;
            put_long(out, i64::try_from(i).unwrap_or(i64::MAX));
            Ok(())
        }
        NamedKind::Fixed(size) => {
            let b = text_bytes(text(v)?)?;
            if u64::try_from(b.len()).ok() != Some(*size) {
                return Err(format!(
                    "fixed {} needs {size} bytes, not {}",
                    n.full,
                    b.len()
                ));
            }
            out.extend(b);
            Ok(())
        }
    }
}

fn write_array(schema: &Schema, items: &Ty, v: &Json, out: &mut Vec<u8>, depth: usize) -> Res<()> {
    let Json::Array(xs) = v else {
        return Err(format!("an array needs a JSON array, not {}", v.to_text()));
    };
    if !xs.is_empty() {
        put_long(out, i64::try_from(xs.len()).unwrap_or(i64::MAX));
        for (i, x) in xs.iter().enumerate() {
            write(schema, items, x, out, depth + 1).map_err(|e| format!("[{i}]: {e}"))?;
        }
    }
    out.push(0);
    Ok(())
}

fn write_map(schema: &Schema, values: &Ty, v: &Json, out: &mut Vec<u8>, depth: usize) -> Res<()> {
    let Json::Object(m) = v else {
        return Err(format!("a map needs a JSON object, not {}", v.to_text()));
    };
    if !m.is_empty() {
        put_long(out, i64::try_from(m.len()).unwrap_or(i64::MAX));
        for (k, x) in m {
            put_bytes(out, k.as_bytes());
            write(schema, values, x, out, depth + 1).map_err(|e| format!("{k}: {e}"))?;
        }
    }
    out.push(0);
    Ok(())
}
