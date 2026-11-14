//! Avro binary to JSON and back, as the Go edition's console showed and sent it (hamba/avro and Avro's JSON encoding): a
//! union's value tagged with its branch (`{"string": "x"}`, null as `null`), bytes and fixed as one character a byte.
//! Logical types are their underlying type (a `timestamp-millis` is the long). A field left out of a value to encode takes
//! its default; a union is tried branch by branch when its value is not tagged.
//!
//! Every length and count in the bytes is checked against what is left before anything is allocated, and the items and
//! nesting are bounded by the [`Budget`]: a record of a few bytes cannot ask for gigabytes or recurse without end.

use super::schema::{NamedKind, Schema, Ty, type_word};
use crate::codec::{Budget, bytes_text, float, text_bytes};
use crate::json::Json;

type Res<T> = Result<T, String>;

/// Avro binary being read.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn left(&self) -> usize {
        self.b.len().saturating_sub(self.at)
    }

    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len());
        let Some(end) = end else {
            return Err(format!(
                "{n} bytes are needed at byte {} and {} are left",
                self.at,
                self.left()
            ));
        };
        let out = self.b.get(self.at..end).ok_or("past the end")?;
        self.at = end;
        Ok(out)
    }

    fn long(&mut self) -> Res<i64> {
        let mut v: u64 = 0;
        for i in 0..10 {
            let [c] = self.take(1)? else {
                return Err("past the end".into());
            };
            v |= u64::from(c & 0x7f) << (7 * i);
            if c & 0x80 == 0 {
                return Ok((v >> 1).cast_signed() ^ -((v & 1).cast_signed()));
            }
        }
        Err(format!("a number at byte {} runs past ten bytes", self.at))
    }

    fn int(&mut self) -> Res<i32> {
        let n = self.long()?;
        i32::try_from(n).map_err(|_| format!("{n} is too large for an int"))
    }

    /// A length or count that must fit in what is left (`each` bytes an item at least).
    fn count(&mut self, what: &str, each: usize) -> Res<usize> {
        let n = self.long()?;
        let n = usize::try_from(n).map_err(|_| format!("a negative {what} ({n})"))?;
        if n.saturating_mul(each) > self.left() {
            return Err(format!(
                "a {what} of {n} is past the end of the bytes ({} left)",
                self.left()
            ));
        }
        Ok(n)
    }

    fn fixed<const N: usize>(&mut self) -> Res<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }
}

/// Reads `body` as `schema`'s root type.
pub(crate) fn decode(schema: &Schema, body: &[u8]) -> Res<Json> {
    let root = schema.root.as_ref().ok_or("the schema has no type")?;
    let mut r = Reader { b: body, at: 0 };
    let mut budget = Budget::default();
    read(schema, root, &mut r, &mut budget)
}

/// The least bytes a value of `t` takes: an item count is checked against it.
fn least(schema: &Schema, t: &Ty) -> usize {
    match t {
        Ty::Null => 0,
        Ty::Float => 4,
        Ty::Double => 8,
        Ty::Named(i) => match schema.named(*i).map(|n| &n.kind) {
            Some(NamedKind::Fixed(n)) => usize::try_from(*n).unwrap_or(usize::MAX),
            Some(NamedKind::Enum { .. }) => 1,
            _ => 0,
        },
        _ => 1,
    }
}

fn read(schema: &Schema, t: &Ty, r: &mut Reader<'_>, budget: &mut Budget) -> Res<Json> {
    budget.take(1)?;
    Ok(match t {
        Ty::Null => Json::Raw("null".into()),
        Ty::Boolean => match r.take(1)? {
            [0] => Json::Raw("false".into()),
            [1] => Json::Raw("true".into()),
            other => return Err(format!("{other:?} is not a boolean")),
        },
        Ty::Int => Json::Raw(r.int()?.to_string()),
        Ty::Long => Json::Raw(r.long()?.to_string()),
        Ty::Float => {
            let f = f32::from_le_bytes(r.fixed::<4>()?);
            if f.is_finite() {
                Json::Raw(format!("{f:?}").trim_end_matches(".0").to_owned())
            } else {
                Json::Str(format!("{f}"))
            }
        }
        Ty::Double => float(f64::from_le_bytes(r.fixed::<8>()?)),
        Ty::Bytes => {
            let n = r.count("length of bytes", 1)?;
            bytes_text(r.take(n)?)
        }
        Ty::String => {
            let n = r.count("string length", 1)?;
            let s = std::str::from_utf8(r.take(n)?).map_err(|e| format!("a string that is not UTF-8 ({e})"))?;
            Json::Str(s.to_owned())
        }
        Ty::Array(items) => budget.deeper(|b| array(schema, items, r, b))?,
        Ty::Map(values) => budget.deeper(|b| map(schema, values, r, b))?,
        Ty::Union(branches) => {
            let i = r.long()?;
            let branch = usize::try_from(i)
                .ok()
                .and_then(|i| branches.get(i))
                .ok_or_else(|| format!("union branch {i} is not one of its {}", branches.len()))?;
            if *branch == Ty::Null {
                Json::Raw("null".into())
            } else {
                let v = budget.deeper(|b| read(schema, branch, r, b))?;
                Json::Object(vec![(branch_name(schema, branch), v)])
            }
        }
        Ty::Named(i) => budget.deeper(|b| named(schema, *i, r, b))?,
    })
}

/// How a branch is named in a tagged union value: its full name, or the type's word.
fn branch_name(schema: &Schema, t: &Ty) -> String {
    match t {
        Ty::Named(i) => schema.named(*i).map(|n| n.full.clone()).unwrap_or_default(),
        other => type_word(other).to_owned(),
    }
}

/// Blocks of items: a count, negative when a byte size follows it, until a count of 0.
fn blocks(
    schema: &Schema,
    item: &Ty,
    r: &mut Reader<'_>,
    budget: &mut Budget,
    mut each: impl FnMut(&mut Reader<'_>, &mut Budget) -> Res<()>,
) -> Res<()> {
    let least = least(schema, item);
    loop {
        let mut n = r.long()?;
        if n == 0 {
            return Ok(());
        }
        if n < 0 {
            n = n.checked_neg().ok_or("a block count that cannot be read")?;
            r.long()?;
        }
        let n = usize::try_from(n).map_err(|_| "a block count that cannot be read")?;
        if n.saturating_mul(least.max(1)) > r.left() && least > 0 {
            return Err(format!("a block of {n} items is past the end of the bytes"));
        }
        budget.take(u64::try_from(n).unwrap_or(u64::MAX))?;
        for _ in 0..n {
            each(r, budget)?;
        }
    }
}

fn array(schema: &Schema, items: &Ty, r: &mut Reader<'_>, budget: &mut Budget) -> Res<Json> {
    let mut out = Vec::new();
    blocks(schema, items, r, budget, |r, b| {
        out.push(read(schema, items, r, b)?);
        Ok(())
    })?;
    Ok(Json::Array(out))
}

fn map(schema: &Schema, values: &Ty, r: &mut Reader<'_>, budget: &mut Budget) -> Res<Json> {
    let mut out = Vec::new();
    blocks(schema, &Ty::String, r, budget, |r, b| {
        let n = r.count("map key length", 1)?;
        let k = std::str::from_utf8(r.take(n)?).map_err(|e| format!("a map key that is not UTF-8 ({e})"))?;
        out.push((k.to_owned(), read(schema, values, r, b)?));
        Ok(())
    })?;
    Ok(Json::Object(out))
}

fn named(schema: &Schema, i: usize, r: &mut Reader<'_>, budget: &mut Budget) -> Res<Json> {
    let n = schema.named(i).ok_or("a type the schema does not define")?;
    match &n.kind {
        NamedKind::Record(fields) => {
            let mut out = Vec::with_capacity(fields.len());
            for f in fields {
                out.push((f.name.clone(), read(schema, &f.ty, r, budget)?));
            }
            Ok(Json::Object(out))
        }
        NamedKind::Enum { symbols, .. } => {
            let i = r.int()?;
            usize::try_from(i)
                .ok()
                .and_then(|i| symbols.get(i))
                .map(|s| Json::Str(s.clone()))
                .ok_or_else(|| format!("enum {} has no symbol {i}", n.full))
        }
        NamedKind::Fixed(size) => {
            let size = usize::try_from(*size).map_err(|_| "a fixed size that cannot be read")?;
            Ok(bytes_text(r.take(size)?))
        }
    }
}

// ---- Encoding ------------------------------------------------------------------------------------------------------

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
        #[expect(clippy::cast_possible_truncation, reason = "a float is the double rounded")]
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
                return Err(format!("record {} needs a JSON object, not {}", n.full, v.to_text()));
            }
            for f in fields {
                let value = match (v.get(&f.name), &f.default) {
                    (Some(x), _) => x,
                    (None, Some(d)) => d,
                    (None, None) => return Err(format!("field {:?} is missing and has no default", f.name)),
                };
                write(schema, &f.ty, value, out, depth + 1).map_err(|e| format!("{}: {e}", f.name))?;
            }
            Ok(())
        }
        NamedKind::Enum { symbols, .. } => {
            let s = text(v)?;
            let i = symbols
                .iter()
                .position(|x| x == s)
                .ok_or_else(|| format!("{s:?} is not a symbol of enum {} ({})", n.full, symbols.join(", ")))?;
            put_long(out, i64::try_from(i).unwrap_or(i64::MAX));
            Ok(())
        }
        NamedKind::Fixed(size) => {
            let b = text_bytes(text(v)?)?;
            if u64::try_from(b.len()).ok() != Some(*size) {
                return Err(format!("fixed {} needs {size} bytes, not {}", n.full, b.len()));
            }
            out.extend(b);
            Ok(())
        }
    }
}

// ---- A sample value -------------------------------------------------------------------------------------------------

/// A value of the schema's shape, every field at its zero (or its default), a union as its first branch.
pub(crate) fn sample(schema: &Schema) -> Json {
    schema
        .root
        .as_ref()
        .map_or_else(|| Json::Raw("null".into()), |t| zero(schema, t, 0))
}

fn zero(schema: &Schema, t: &Ty, depth: usize) -> Json {
    let raw = |s: &str| Json::Raw(s.to_owned());
    match t {
        Ty::Null => raw("null"),
        Ty::Boolean => raw("false"),
        Ty::Int | Ty::Long | Ty::Float | Ty::Double => raw("0"),
        Ty::Bytes | Ty::String => Json::Str(String::new()),
        Ty::Array(_) => Json::Array(Vec::new()),
        Ty::Map(_) => Json::Object(Vec::new()),
        Ty::Union(bs) => bs.first().map_or_else(|| raw("null"), |b| zero(schema, b, depth)),
        Ty::Named(i) => match schema.named(*i).map(|n| &n.kind) {
            Some(NamedKind::Record(fields)) if depth < 6 => Json::Object(
                fields
                    .iter()
                    .map(|f| {
                        let v = f.default.clone().unwrap_or_else(|| zero(schema, &f.ty, depth + 1));
                        (f.name.clone(), v)
                    })
                    .collect(),
            ),
            Some(NamedKind::Enum { symbols, .. }) => Json::Str(symbols.first().cloned().unwrap_or_default()),
            Some(NamedKind::Fixed(n)) => Json::Str("0".repeat(usize::try_from(*n).unwrap_or(0).min(64))),
            _ => raw("null"),
        },
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
