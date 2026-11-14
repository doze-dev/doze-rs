//! Protobuf binary to JSON and back, written here (no Protobuf runtime is linked), as protojson writes and reads it: fields by
//! their JSON names (`order_id` is `orderId`; either is read), 64-bit integers as strings, bytes as base64, enums by their
//! value names, maps as objects; fields not on the wire are left out. A value's message index (after the wire header) names
//! the message; encoding writes the first message, as the Go edition did, with Confluent's single 0 for it.
//!
//! Every length is checked against what is left, groups and messages nest at most [`MAX_DEPTH`], and the items a value
//! makes are bounded by the [`Budget`]. Fields the schema does not know are skipped, as a Protobuf reader does. The
//! well-known types (`Timestamp`, `Duration`, the wrappers) are shown as the messages they are, not in protojson's
//! special forms.

use super::link::{Card, Fld, Msg, Pool, Ty, json_name};
use crate::codec::{Budget, float};
use crate::json::Json;
use kafka_auth::crypto::base64;

type Res<T> = Result<T, String>;

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn done(&self) -> bool {
        self.at >= self.b.len()
    }

    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len());
        let Some(end) = end else {
            return Err(format!(
                "{n} bytes are needed at byte {} and {} are left",
                self.at,
                self.b.len().saturating_sub(self.at)
            ));
        };
        let out = self.b.get(self.at..end).ok_or("past the end")?;
        self.at = end;
        Ok(out)
    }

    fn varint(&mut self) -> Res<u64> {
        let mut v: u64 = 0;
        for i in 0..10 {
            let [c] = self.take(1)? else {
                return Err("past the end".into());
            };
            v |= u64::from(c & 0x7f) << (7 * i);
            if c & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(format!("a varint at byte {} runs past ten bytes", self.at))
    }

    fn len(&mut self) -> Res<&'a [u8]> {
        let n = usize::try_from(self.varint()?).map_err(|_| "a length that cannot be read")?;
        self.take(n)
    }
}

/// Reads `body`: the message index, then the message it names.
pub(super) fn decode(pool: &Pool, file: usize, body: &[u8]) -> Res<(Json, String)> {
    let (idx, rest) = crate::wire::message_indexes(body).map_err(|e| e.to_string())?;
    let m = message_at(pool, file, &idx)
        .ok_or_else(|| format!("message index {idx:?} is outside the schema"))?;
    let mut budget = Budget::default();
    let json = message(pool, m, &mut Reader { b: rest, at: 0 }, None, &mut budget)?;
    Ok((json, m.full.clone()))
}

fn message_at<'p>(pool: &'p Pool, file: usize, idx: &[usize]) -> Option<&'p Msg> {
    let (first, rest) = idx.split_first()?;
    let mut m = pool.msg(*pool.files.get(file)?.msgs.get(*first)?)?;
    for i in rest {
        m = pool.msg(*m.nested.get(*i)?)?;
    }
    Some(m)
}

/// A message's fields, until the bytes end (or, for a group, its end tag).
fn message(
    pool: &Pool,
    m: &Msg,
    r: &mut Reader<'_>,
    group: Option<u64>,
    budget: &mut Budget,
) -> Res<Json> {
    let mut out: Vec<(String, Json)> = Vec::new();
    while !r.done() {
        let tag = r.varint()?;
        let (number, wire) = (tag >> 3, tag & 7);
        if wire == 4 {
            return if group == Some(number) {
                Ok(Json::Object(out))
            } else {
                Err(format!("an end of group {number} that was not begun"))
            };
        }
        budget.take(1)?;
        let field = m
            .fields
            .iter()
            .find(|f| u64::try_from(f.number).ok() == Some(number));
        let Some(f) = field else {
            skip(r, wire, number, budget)?;
            continue;
        };
        let values = budget.deeper(|b| field_values(pool, f, wire, number, r, b))?;
        put(&mut out, pool, f, values);
    }
    if let Some(n) = group {
        return Err(format!("group {n} has no end"));
    }
    Ok(Json::Object(out))
}

/// Adds a field's values: a repeated field gathers them (a map's entries as an object), any other keeps the last.
fn put(out: &mut Vec<(String, Json)>, pool: &Pool, f: &Fld, values: Vec<Json>) {
    let name = json_name(&f.name);
    let map = matches!(f.ty, Ty::Message(i) if pool.msg(i).is_some_and(|m| m.map_entry));
    let slot = out.iter().position(|(k, _)| *k == name);
    if f.card != Card::Repeated {
        let Some(v) = values.into_iter().last() else {
            return;
        };
        match slot.and_then(|i| out.get_mut(i)) {
            Some((_, old)) => *old = v,
            None => out.push((name, v)),
        }
        return;
    }
    let i = slot.unwrap_or_else(|| {
        out.push((
            name,
            if map {
                Json::Object(Vec::new())
            } else {
                Json::Array(Vec::new())
            },
        ));
        out.len() - 1
    });
    match out.get_mut(i).map(|(_, v)| v) {
        Some(Json::Object(entries)) => {
            for e in values {
                let key = match e.get("key") {
                    Some(Json::Str(s)) => s.clone(),
                    Some(Json::Raw(r)) => r.clone(),
                    _ => String::new(),
                };
                let value = e.get("value").cloned().unwrap_or(Json::Raw("null".into()));
                entries.push((key, value));
            }
        }
        Some(Json::Array(items)) => items.extend(values),
        _ => {}
    }
}

fn field_values(
    pool: &Pool,
    f: &Fld,
    wire: u64,
    number: u64,
    r: &mut Reader<'_>,
    budget: &mut Budget,
) -> Res<Vec<Json>> {
    match f.ty {
        Ty::Message(i) => {
            expect(wire, 2, f)?;
            let m = pool.msg(i).ok_or("a message the schema does not define")?;
            let body = r.len()?;
            Ok(vec![message(
                pool,
                m,
                &mut Reader { b: body, at: 0 },
                None,
                budget,
            )?])
        }
        Ty::Group(i) => {
            expect(wire, 3, f)?;
            let m = pool.msg(i).ok_or("a group the schema does not define")?;
            Ok(vec![message(pool, m, r, Some(number), budget)?])
        }
        Ty::Enum(e) => {
            let one = |r: &mut Reader<'_>| -> Res<Json> {
                let n = enum_number(r.varint()?);
                let info = pool.enums.get(e);
                Ok(info
                    .and_then(|i| {
                        i.numbers
                            .iter()
                            .position(|x| *x == n)
                            .and_then(|p| i.values.get(p))
                    })
                    .map_or_else(|| Json::Raw(n.to_string()), |name| Json::Str(name.clone())))
            };
            packed_or_one(wire, 0, r, budget, one)
        }
        Ty::Scalar(t) => {
            let natural = natural_wire(t);
            if natural == 2 {
                expect(wire, 2, f)?;
                let b = r.len()?;
                return Ok(vec![if t == 9 {
                    Json::Str(
                        std::str::from_utf8(b)
                            .map_err(|e| format!("{}: a string that is not UTF-8 ({e})", f.name))?
                            .to_owned(),
                    )
                } else {
                    Json::Str(base64::encode(b))
                }]);
            }
            packed_or_one(wire, natural, r, budget, |r| scalar(t, r))
        }
    }
}

fn expect(wire: u64, want: u64, f: &Fld) -> Res<()> {
    if wire == want {
        Ok(())
    } else {
        Err(format!(
            "field {} ({}) has wire type {wire}, not {want}",
            f.name, f.number
        ))
    }
}

/// One value in its natural wire type, or a packed run of them (wire type 2).
fn packed_or_one(
    wire: u64,
    natural: u64,
    r: &mut Reader<'_>,
    budget: &mut Budget,
    mut one: impl FnMut(&mut Reader<'_>) -> Res<Json>,
) -> Res<Vec<Json>> {
    if wire == natural {
        return Ok(vec![one(r)?]);
    }
    if wire != 2 {
        return Err(format!(
            "wire type {wire} where {natural} or a packed run was expected"
        ));
    }
    let b = r.len()?;
    let mut inner = Reader { b, at: 0 };
    let mut out = Vec::new();
    while !inner.done() {
        budget.take(1)?;
        out.push(one(&mut inner)?);
    }
    Ok(out)
}

/// The wire type a scalar is written with: 0 varint, 1 eight bytes, 5 four bytes, 2 length-delimited.
pub(super) fn natural_wire(t: u8) -> u64 {
    match t {
        1 | 6 | 16 => 1,
        2 | 7 | 15 => 5,
        9 | 12 => 2,
        _ => 0,
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "Protobuf's 32-bit varints are the low bits of the 64"
)]
fn scalar(t: u8, r: &mut Reader<'_>) -> Res<Json> {
    let fixed4 = |r: &mut Reader<'_>| -> Res<[u8; 4]> {
        r.take(4)?.try_into().map_err(|_| "four bytes".to_owned())
    };
    let fixed8 = |r: &mut Reader<'_>| -> Res<[u8; 8]> {
        r.take(8)?.try_into().map_err(|_| "eight bytes".to_owned())
    };
    let raw = |s: String| Json::Raw(s);
    Ok(match t {
        1 => float(f64::from_le_bytes(fixed8(r)?)),
        2 => float(f64::from(f32::from_le_bytes(fixed4(r)?))),
        3 => Json::Str(r.varint()?.cast_signed().to_string()),
        4 => Json::Str(r.varint()?.to_string()),
        5 => raw((r.varint()? as i32).to_string()),
        6 => Json::Str(u64::from_le_bytes(fixed8(r)?).to_string()),
        7 => raw(u32::from_le_bytes(fixed4(r)?).to_string()),
        8 => raw((r.varint()? != 0).to_string()),
        13 => raw((r.varint()? as u32).to_string()),
        15 => raw(i32::from_le_bytes(fixed4(r)?).to_string()),
        16 => Json::Str(i64::from_le_bytes(fixed8(r)?).to_string()),
        17 => {
            let v = r.varint()? as u32;
            raw(((v >> 1).cast_signed() ^ -((v & 1).cast_signed())).to_string())
        }
        18 => {
            let v = r.varint()?;
            Json::Str(((v >> 1).cast_signed() ^ -((v & 1).cast_signed())).to_string())
        }
        other => return Err(format!("scalar type {other} is not one Protobuf has")),
    })
}

fn skip(r: &mut Reader<'_>, wire: u64, number: u64, budget: &mut Budget) -> Res<()> {
    match wire {
        0 => r.varint().map(drop),
        1 => r.take(8).map(drop),
        2 => r.len().map(drop),
        5 => r.take(4).map(drop),
        3 => budget.deeper(|b| {
            loop {
                let tag = r.varint()?;
                if tag & 7 == 4 {
                    return if tag >> 3 == number {
                        Ok(())
                    } else {
                        Err("a group ends where another began".into())
                    };
                }
                b.take(1)?;
                skip(r, tag & 7, tag >> 3, b)?;
            }
        }),
        other => Err(format!("wire type {other} is not one Protobuf has")),
    }
}

// ---- Encoding ------------------------------------------------------------------------------------------------------

/// Writes `value` as the file's first message, after its message index (a single 0).
pub(super) fn encode(pool: &Pool, file: usize, value: &Json) -> Res<Vec<u8>> {
    let m = message_at(pool, file, &[0]).ok_or("the schema declares no message")?;
    let mut out = vec![0];
    write_message(pool, m, value, &mut out, 0)?;
    Ok(out)
}

use super::encode::write_message;

/// An enum value as it is on the wire: an int32, sign-extended to ten bytes when negative.
#[expect(
    clippy::cast_possible_truncation,
    reason = "an enum number is the low 32 bits"
)]
fn enum_number(v: u64) -> i64 {
    i64::from(v as i32)
}

#[cfg(test)]
mod tests {
    use super::base64;

    /// Protojson writes `bytes` as standard base64 with padding (the shared encoder of `kafka-auth`).
    #[test]
    fn bytes_are_base64() {
        assert_eq!(base64::encode(b""), "");
        assert_eq!(base64::encode(b"h"), "aA==");
        assert_eq!(base64::encode(b"hi"), "aGk=");
        assert_eq!(base64::encode(b"hello"), "aGVsbG8=");
    }
}
