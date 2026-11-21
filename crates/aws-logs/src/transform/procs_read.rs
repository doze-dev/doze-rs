//! The readers `build` uses: one processor field at a time, checked and turned into its typed form.

use aws_cron::Zone;
use aws_wire::ApiError;
use regex_lite::{Regex, RegexBuilder};
use serde_json::{Map, Value};

use super::datetime::{self};
use super::procs::{Csv, DateTime, KeyValue, Kind, ListToMap, MAX_REGEX_BYTES, Move, invalid};

pub(super) fn opt(b: &Map<String, Value>, key: &str) -> Option<String> {
    b.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub(super) fn need(b: &Map<String, Value>, key: &str) -> Result<String, ApiError> {
    opt(b, key)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(format!("{key} is required.")))
}

pub(super) fn flag(b: &Map<String, Value>, key: &str) -> bool {
    b.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(super) fn entries<'a>(
    b: &'a Map<String, Value>,
    key: &str,
) -> Result<Vec<&'a Map<String, Value>>, ApiError> {
    let list = b
        .get(key)
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| invalid(format!("{key} is required.")))?;
    list.iter()
        .map(|e| {
            e.as_object()
                .ok_or_else(|| invalid(format!("Each of {key} must be an object.")))
        })
        .collect()
}

pub(super) fn strings(b: &Map<String, Value>, key: &str) -> Result<Vec<String>, ApiError> {
    let list = b
        .get(key)
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| invalid(format!("{key} is required.")))?;
    list.iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("Each of {key} must be a string.")))
        })
        .collect()
}

pub(super) fn moves(b: &Map<String, Value>, from: &str, to: &str) -> Result<Vec<Move>, ApiError> {
    entries(b, "entries")?
        .iter()
        .map(|e| {
            Ok(Move {
                source: need(e, from)?,
                target: need(e, to)?,
                overwrite: flag(e, "overwriteIfExists"),
            })
        })
        .collect()
}

pub(super) fn csv(b: &Map<String, Value>) -> Result<Csv, ApiError> {
    let quote = opt(b, "quoteCharacter").unwrap_or_else(|| "\"".into());
    let mut q = quote.chars();
    let (Some(quote), None) = (q.next(), q.next()) else {
        return Err(invalid("quoteCharacter must be one character."));
    };
    let delimiter = match opt(b, "delimiter").as_deref() {
        None => ",".to_owned(),
        // (memory) a tab may be written as the two characters `\t`.
        Some("\\t") => "\t".to_owned(),
        Some(d) => d.to_owned(),
    };
    if delimiter.is_empty() {
        return Err(invalid("delimiter must not be empty."));
    }
    let columns = match b.get("columns") {
        Some(_) => strings(b, "columns").or_else(|_| Ok::<_, ApiError>(Vec::new()))?,
        None => Vec::new(),
    };
    Ok(Csv {
        quote,
        delimiter,
        columns,
        source: opt(b, "source"),
        destination: opt(b, "destination"),
    })
}

pub(super) fn zone(b: &Map<String, Value>, key: &str) -> Result<Zone, ApiError> {
    match opt(b, key) {
        None => Ok(Zone::utc()),
        Some(name) => Zone::named(&name)
            .ok_or_else(|| invalid(format!("{key} {name} is not a known time zone."))),
    }
}

pub(super) fn date_time(b: &Map<String, Value>) -> Result<DateTime, ApiError> {
    let read = |text: &str| {
        datetime::pattern(text)
            .ok_or_else(|| invalid(format!("The date time pattern {text} is not valid.")))
    };
    let patterns = strings(b, "matchPatterns")?
        .iter()
        .map(|p| read(p))
        .collect::<Result<Vec<_>, _>>()?;
    let target_format =
        read(&opt(b, "targetFormat").unwrap_or_else(|| "yyyy-MM-dd'T'HH:mm:ss.SSS'Z'".into()))?;
    Ok(DateTime {
        source: need(b, "source")?,
        target: need(b, "target")?,
        target_format,
        patterns,
        source_zone: zone(b, "sourceTimezone")?,
        target_zone: zone(b, "targetTimezone")?,
    })
}

pub(super) fn list_to_map(b: &Map<String, Value>) -> Result<ListToMap, ApiError> {
    let flatten = if flag(b, "flatten") {
        match opt(b, "flattenedElement").as_deref() {
            Some("first") => Some(true),
            Some("last") => Some(false),
            _ => {
                return Err(invalid(
                    "flattenedElement must be first or last when flatten is true.",
                ));
            }
        }
    } else {
        None
    };
    Ok(ListToMap {
        source: need(b, "source")?,
        key: need(b, "key")?,
        value_key: opt(b, "valueKey"),
        target: opt(b, "target"),
        flatten,
    })
}

pub(super) fn key_value(b: &Map<String, Value>) -> KeyValue {
    let or = |key: &str, default: &str| {
        opt(b, key)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| default.to_owned())
    };
    KeyValue {
        source: opt(b, "source"),
        destination: opt(b, "destination"),
        field_delimiter: or("fieldDelimiter", "&"),
        kv_delimiter: or("keyValueDelimiter", "="),
        prefix: opt(b, "keyPrefix").unwrap_or_default(),
        non_match: opt(b, "nonMatchValue"),
        overwrite: flag(b, "overwriteIfExists"),
    }
}

pub(super) fn substitutions(
    b: &Map<String, Value>,
) -> Result<Vec<(String, Regex, String)>, ApiError> {
    entries(b, "entries")?
        .iter()
        .map(|e| {
            let from = need(e, "from")?;
            let re = RegexBuilder::new(&from)
                .size_limit(MAX_REGEX_BYTES)
                .build()
                .map_err(|_| invalid(format!("The regular expression {from} is not valid.")))?;
            Ok((need(e, "source")?, re, opt(e, "to").unwrap_or_default()))
        })
        .collect()
}

pub(super) fn conversions(b: &Map<String, Value>) -> Result<Vec<(String, Kind)>, ApiError> {
    entries(b, "entries")?
        .iter()
        .map(|e| {
            let kind = match need(e, "type")?.as_str() {
                "integer" => Kind::Integer,
                "double" => Kind::Double,
                "string" => Kind::Text,
                "boolean" => Kind::Boolean,
                other => {
                    return Err(invalid(format!(
                        "The type {other} is not valid. Valid values are integer, double, string and boolean."
                    )));
                }
            };
            Ok((need(e, "key")?, kind))
        })
        .collect()
}
