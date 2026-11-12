//! Decoding a JSON object into a record the way Go's `encoding/json` decodes it into a struct, so a line or file the
//! Go doze accepts is accepted here, and one it rejects is rejected:
//!
//! - keys match fields case-insensitively (ASCII), and every key is applied in order, so a repeated key's last value
//!   wins and a repeated object merges into the one before;
//! - `null` for a field leaves it as it was (for a list it empties it), and `null` as the whole document is an empty
//!   record;
//! - a value of the wrong type (a string for a number, a fraction for an integer) fails the whole decode;
//! - unknown keys are ignored.
//!
//! Invalid UTF-8 inside strings becomes U+FFFD, as in Go. The same file is in doze-projects and doze-journal: the two
//! crates have no common dependency to hold it, and it is too small to be a crate of its own.

use serde::de::{Deserializer, Error as _, MapAccess, Visitor};
use serde_json::value::RawValue;
use std::fmt;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// An object's keys and undecoded values, in document order. `None` is a `null` document.
pub(crate) type Pairs<'a> = Option<Vec<(String, &'a RawValue)>>;

/// Splits a document into its object's pairs. Anything but an object or `null` is an error, as Go's is.
pub(crate) fn object(text: &str) -> serde_json::Result<Pairs<'_>> {
    let mut de = serde_json::Deserializer::from_str(text);
    let pairs = (&mut de).deserialize_any(ObjectVisitor)?;
    de.end()?;
    Ok(pairs)
}

struct ObjectVisitor;

impl<'de> Visitor<'de> for ObjectVisitor {
    type Value = Pairs<'de>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object or null")
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut out = Vec::new();
        while let Some(pair) = map.next_entry::<String, &'de RawValue>()? {
            out.push(pair);
        }
        Ok(Some(out))
    }
}

/// Whether a key names a field, as Go matches them.
pub(crate) fn is(key: &str, field: &str) -> bool {
    key.eq_ignore_ascii_case(field)
}

/// A value of any type serde reads; `None` for `null`.
pub(crate) fn value<'a, T: serde::Deserialize<'a>>(
    raw: &'a RawValue,
) -> serde_json::Result<Option<T>> {
    serde_json::from_str(raw.get())
}

#[allow(
    dead_code,
    reason = "the same file serves doze-journal, whose events have no lists"
)]
/// A list of strings: `null` empties it, and a `null` element is an empty string.
pub(crate) fn strings(raw: &RawValue) -> serde_json::Result<Vec<String>> {
    let list: Option<Vec<Option<String>>> = value(raw)?;
    Ok(list
        .unwrap_or_default()
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect())
}

/// A Go `time.Time`: an RFC 3339 string. Go's zero time (year 1, what Go writes for "never") is `None`.
pub(crate) fn time(raw: &RawValue) -> serde_json::Result<Option<OffsetDateTime>> {
    let Some(s) = value::<String>(raw)? else {
        return Ok(None);
    };
    // Go takes only an upper-case `T` between date and time (not the lower case or space RFC 3339 also allows), only
    // an upper-case `Z`, and no leap second (which the time crate reads as the last instant before it).
    if s.get(10..11) != Some("T") || s.contains('z') || s.get(17..19) == Some("60") {
        return Err(serde_json::Error::custom(format!(
            "time {s:?}: not a time Go reads"
        )));
    }
    // Go takes a comma before the fraction too.
    let s = if s.get(19..20) == Some(",") {
        s.replacen(',', ".", 1)
    } else {
        s
    };
    OffsetDateTime::parse(&s, &Rfc3339)
        .map(|t| (t != go_zero()).then_some(t))
        .map_err(|e| serde_json::Error::custom(format!("time {s:?}: {e}")))
}

/// Go's zero `time.Time`, 0001-01-01T00:00:00Z: the same instant at any offset.
fn go_zero() -> OffsetDateTime {
    time::PrimitiveDateTime::new(
        time::Date::MIN.replace_year(1).unwrap_or(time::Date::MIN),
        time::Time::MIDNIGHT,
    )
    .assume_utc()
}

/// The text with each `\u` escape of a lone UTF-16 surrogate made `�`, which Go decodes it to (serde_json
/// rejects it). Only escapes are touched: a backslash outside a string is a syntax error either way.
pub(crate) fn mend_surrogates(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("\\u") {
        return text.into();
    }
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut from) = (0, 0);
    while let Some(&c) = b.get(i) {
        if c != b'\\' {
            i += 1;
            continue;
        }
        let Some(unit) = escape(b, i) else {
            i += 2; // `\\`, `\"` and the like: never the start of a `\u`
            continue;
        };
        let pair = (0xD800..0xDC00).contains(&unit)
            && escape(b, i + 6).is_some_and(|n| (0xDC00..0xE000).contains(&n));
        if pair {
            i += 12;
        } else if (0xD800..0xE000).contains(&unit) {
            out.push_str(text.get(from..i).unwrap_or_default());
            out.push_str("\\ufffd");
            i += 6;
            from = i;
        } else {
            i += 6;
        }
    }
    out.push_str(text.get(from..).unwrap_or_default());
    out.into()
}

/// The code unit of a `\uXXXX` escape at `at`.
fn escape(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 6)?;
    let hex = s.strip_prefix(b"\\u")?;
    u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()
}
