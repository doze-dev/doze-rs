//! JSON to Protobuf binary, as protojson reads it: a field by its JSON name or its own, integers as numbers or strings,
//! enums by name or number, bytes as base64, maps as objects, `null` as the field left out. Repeated numbers are written
//! packed (every Protobuf reader takes packed and unpacked alike).

use super::link::{Card, Fld, Msg, Pool, Ty, json_name};
use crate::codec::MAX_DEPTH;
use crate::json::Json;

type Res<T> = Result<T, String>;

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(u8::try_from(v & 0x7f).unwrap_or(0) | 0x80);
        v >>= 7;
    }
    out.push(u8::try_from(v).unwrap_or(0));
}

fn put_tag(out: &mut Vec<u8>, number: i32, wire: u64) {
    put_varint(out, (u64::from(number.cast_unsigned()) << 3) | wire);
}

fn put_len(out: &mut Vec<u8>, b: &[u8]) {
    put_varint(out, u64::try_from(b.len()).unwrap_or(u64::MAX));
    out.extend_from_slice(b);
}

fn is_null(v: &Json) -> bool {
    matches!(v, Json::Raw(r) if r == "null")
}

/// Writes `value`, an object, as message `m`.
pub(super) fn write_message(pool: &Pool, m: &Msg, value: &Json, out: &mut Vec<u8>, depth: usize) -> Res<()> {
    if depth > MAX_DEPTH {
        return Err("the value nests too deep".into());
    }
    let Json::Object(members) = value else {
        return Err(format!("message {} needs a JSON object, not {}", m.full, value.to_text()));
    };
    for (k, v) in members {
        let f = m
            .fields
            .iter()
            .find(|f| json_name(&f.name) == *k || f.name == *k)
            .ok_or_else(|| format!("{} has no field {k:?}", m.full))?;
        if is_null(v) {
            continue;
        }
        field(pool, f, v, out, depth).map_err(|e| format!("{k}: {e}"))?;
    }
    Ok(())
}

fn field(pool: &Pool, f: &Fld, v: &Json, out: &mut Vec<u8>, depth: usize) -> Res<()> {
    if f.card != Card::Repeated {
        return one(pool, f, v, out, depth);
    }
    if let Ty::Message(i) = f.ty
        && let Some(entry) = pool.msg(i).filter(|e| e.map_entry)
    {
        let Json::Object(entries) = v else {
            return Err(format!("a map needs a JSON object, not {}", v.to_text()));
        };
        for (k, x) in entries {
            let pair = Json::Object(vec![("key".into(), map_key(entry, k)), ("value".into(), x.clone())]);
            let mut body = Vec::new();
            write_message(pool, entry, &pair, &mut body, depth + 1).map_err(|e| format!("{k}: {e}"))?;
            put_tag(out, f.number, 2);
            put_len(out, &body);
        }
        return Ok(());
    }
    let Json::Array(items) = v else {
        return Err(format!("a repeated field needs a JSON array, not {}", v.to_text()));
    };
    let packs = matches!(f.ty, Ty::Enum(_)) || matches!(f.ty, Ty::Scalar(t) if t != 9 && t != 12);
    if packs {
        let mut body = Vec::new();
        for (i, x) in items.iter().enumerate() {
            value(pool, f, x, &mut body).map_err(|e| format!("[{i}]: {e}"))?;
        }
        if !items.is_empty() {
            put_tag(out, f.number, 2);
            put_len(out, &body);
        }
        return Ok(());
    }
    for (i, x) in items.iter().enumerate() {
        one(pool, f, x, out, depth).map_err(|e| format!("[{i}]: {e}"))?;
    }
    Ok(())
}

/// A map's key, as its entry's key field takes it: a JSON object key is always text.
fn map_key(entry: &Msg, k: &str) -> Json {
    let text_key = entry
        .fields
        .iter()
        .find(|f| f.number == 1)
        .is_some_and(|f| matches!(f.ty, Ty::Scalar(9)));
    if text_key { Json::Str(k.to_owned()) } else { Json::Raw(k.to_owned()) }
}

/// One value of a field, with its tag.
fn one(pool: &Pool, f: &Fld, v: &Json, out: &mut Vec<u8>, depth: usize) -> Res<()> {
    match f.ty {
        Ty::Message(i) => {
            let m = pool.msg(i).ok_or("a message the schema does not define")?;
            let mut body = Vec::new();
            write_message(pool, m, v, &mut body, depth + 1)?;
            put_tag(out, f.number, 2);
            put_len(out, &body);
        }
        Ty::Group(i) => {
            let m = pool.msg(i).ok_or("a group the schema does not define")?;
            put_tag(out, f.number, 3);
            write_message(pool, m, v, out, depth + 1)?;
            put_tag(out, f.number, 4);
        }
        Ty::Scalar(9) => {
            put_tag(out, f.number, 2);
            put_len(out, v.as_str().ok_or_else(|| format!("expected a string, not {}", v.to_text()))?.as_bytes());
        }
        Ty::Scalar(12) => {
            let s = v.as_str().ok_or_else(|| format!("expected base64 text, not {}", v.to_text()))?;
            put_tag(out, f.number, 2);
            put_len(out, &from_base64(s)?);
        }
        Ty::Scalar(t) => {
            put_tag(out, f.number, super::codec::natural_wire(t));
            value(pool, f, v, out)?;
        }
        Ty::Enum(_) => {
            put_tag(out, f.number, 0);
            value(pool, f, v, out)?;
        }
    }
    Ok(())
}

fn from_base64(s: &str) -> Res<Vec<u8>> {
    let std: String = s
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    let padded = format!("{std}{}", "=".repeat((4 - std.len() % 4) % 4));
    crate::codec::base64_bytes(&padded).ok_or_else(|| format!("{s:?} is not base64"))
}

fn number_text(v: &Json) -> Res<&str> {
    match v {
        Json::Raw(r) if !matches!(r.as_str(), "true" | "false" | "null") => Ok(r),
        Json::Str(s) => Ok(s),
        _ => Err(format!("expected a number, not {}", v.to_text())),
    }
}

fn int<T: std::str::FromStr>(v: &Json, what: &str) -> Res<T> {
    let t = number_text(v)?;
    t.parse::<T>().map_err(|_| format!("{t} is not {what}"))
}

fn real(v: &Json) -> Res<f64> {
    let t = number_text(v)?;
    match t {
        "NaN" => Ok(f64::NAN),
        "Infinity" => Ok(f64::INFINITY),
        "-Infinity" => Ok(f64::NEG_INFINITY),
        _ => t.parse::<f64>().map_err(|_| format!("{t} is not a number")),
    }
}

/// A value without its tag (a packed run is these one after another).
fn value(pool: &Pool, f: &Fld, v: &Json, out: &mut Vec<u8>) -> Res<()> {
    let zigzag = |n: i64| ((n << 1) ^ (n >> 63)).cast_unsigned();
    match f.ty {
        Ty::Enum(e) => {
            let info = pool.enums.get(e).ok_or("an enum the schema does not define")?;
            let n = match v {
                Json::Str(name) => match info.values.iter().position(|x| x == name) {
                    Some(p) => info.numbers.get(p).copied().unwrap_or(0),
                    None => return Err(format!("{name:?} is not a value of the enum ({})", info.values.join(", "))),
                },
                other => int::<i32>(other, "an enum number").map(i64::from)?,
            };
            put_varint(out, n.cast_unsigned());
        }
        Ty::Scalar(t) => match t {
            1 => out.extend(real(v)?.to_le_bytes()),
            #[expect(clippy::cast_possible_truncation, reason = "a float is the double rounded")]
            2 => out.extend((real(v)? as f32).to_le_bytes()),
            3 => put_varint(out, int::<i64>(v, "an int64")?.cast_unsigned()),
            4 => put_varint(out, int::<u64>(v, "a uint64")?),
            5 => put_varint(out, i64::from(int::<i32>(v, "an int32")?).cast_unsigned()),
            6 => out.extend(int::<u64>(v, "a fixed64")?.to_le_bytes()),
            7 => out.extend(int::<u32>(v, "a fixed32")?.to_le_bytes()),
            8 => match v {
                Json::Raw(b) if b == "true" || b == "false" => out.push(u8::from(b == "true")),
                _ => return Err(format!("expected true or false, not {}", v.to_text())),
            },
            13 => put_varint(out, u64::from(int::<u32>(v, "a uint32")?)),
            15 => out.extend(int::<i32>(v, "an sfixed32")?.to_le_bytes()),
            16 => out.extend(int::<i64>(v, "an sfixed64")?.to_le_bytes()),
            17 => put_varint(out, zigzag(i64::from(int::<i32>(v, "an sint32")?)) & 0xffff_ffff),
            18 => put_varint(out, zigzag(int::<i64>(v, "an sint64")?)),
            other => return Err(format!("scalar type {other} cannot be packed")),
        },
        _ => return Err("not a packable field".into()),
    }
    Ok(())
}
