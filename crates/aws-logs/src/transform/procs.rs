//! A transformer's processors as the request writes them (`transformerConfig`), checked and read into [`Proc`].

use aws_cron::Zone;
use aws_wire::ApiError;
use regex_lite::{Regex, RegexBuilder};
use serde_json::{Map, Value};

use super::datetime::{self, Pattern};
use super::grok::{self, Grok};

/// The most processors a transformer has (the model's `Processors` length).
pub(crate) const MAX_PROCESSORS: usize = 20;
/// The most compiled size of a `substituteString` expression.
const MAX_REGEX_BYTES: usize = 1 << 20;
/// The parsers of AWS-vended logs, which must come first and read only `@message` (the model).
const VENDED: [&str; 5] = [
    "parseCloudfront",
    "parseRoute53",
    "parsePostgres",
    "parseVPC",
    "parseWAF",
];

/// The processors a transformer may begin with, in the order AWS lists them (recorded 2026-11-19).
const PARSERS: [&str; 19] = [
    "parsePostgres",
    "parseWAF",
    "parseCloudfront",
    "parseCloudfrontOCSF",
    "parseRoute53",
    "parseLambda",
    "parseVPC",
    "parseSyslog",
    "parseRDS",
    "parseCiscoFTD",
    "parseF5BigIP",
    "parseCiscoISE",
    "parseForescoutNac",
    "parseInfobloxDDI",
    "parseToOCSF",
    "parseJSON",
    "grok",
    "csv",
    "parseKeyValue",
];

/// The processor members the model gives a default, as AWS shows them in `GetTransformer` (recorded 2026-11-19 for `addKeys`
/// and `renameKeys`; the others by the same model trait): `(processor, inside entries, member, default)`.
const DEFAULTS: [(&str, bool, &str, bool); 6] = [
    ("addKeys", true, "overwriteIfExists", false),
    ("copyValue", true, "overwriteIfExists", false),
    ("moveKeys", true, "overwriteIfExists", false),
    ("renameKeys", true, "overwriteIfExists", false),
    ("listToMap", false, "flatten", false),
    ("parseKeyValue", false, "overwriteIfExists", false),
];

/// A stored `transformerConfig` with each member the model defaults filled in where the request left it out.
pub(crate) fn with_defaults(config: &mut Value) {
    let Some(list) = config.as_array_mut() else {
        return;
    };
    for p in list.iter_mut().filter_map(Value::as_object_mut) {
        for (name, in_entries, member, default) in DEFAULTS {
            let Some(body) = p.get_mut(name).and_then(Value::as_object_mut) else {
                continue;
            };
            let mut fill = |o: &mut Map<String, Value>| {
                o.entry(member).or_insert(Value::Bool(default));
            };
            if in_entries {
                body.get_mut("entries")
                    .and_then(Value::as_array_mut)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_object_mut)
                    .for_each(&mut fill);
            } else {
                fill(body);
            }
        }
    }
}

/// One key moved, copied or renamed.
#[derive(Clone, Debug)]
pub(crate) struct Move {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) overwrite: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct DateTime {
    pub(crate) source: String,
    pub(crate) target: String,
    pub(crate) target_format: Pattern,
    pub(crate) patterns: Vec<Pattern>,
    pub(crate) source_zone: Zone,
    pub(crate) target_zone: Zone,
}

#[derive(Clone, Debug)]
pub(crate) struct KeyValue {
    pub(crate) source: Option<String>,
    pub(crate) destination: Option<String>,
    pub(crate) field_delimiter: String,
    pub(crate) kv_delimiter: String,
    pub(crate) prefix: String,
    pub(crate) non_match: Option<String>,
    pub(crate) overwrite: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Csv {
    pub(crate) quote: char,
    pub(crate) delimiter: String,
    pub(crate) columns: Vec<String>,
    pub(crate) source: Option<String>,
    pub(crate) destination: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ListToMap {
    pub(crate) source: String,
    pub(crate) key: String,
    pub(crate) value_key: Option<String>,
    pub(crate) target: Option<String>,
    /// `Some(true)` keeps the first element, `Some(false)` the last; `None` keeps them all in a list.
    pub(crate) flatten: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Integer,
    Double,
    Text,
    Boolean,
}

#[derive(Clone, Debug)]
pub(crate) enum Proc {
    AddKeys(Vec<(String, String, bool)>),
    CopyValue(Vec<Move>),
    MoveKeys(Vec<Move>),
    RenameKeys(Vec<Move>),
    Csv(Box<Csv>),
    DateTime(Box<DateTime>),
    DeleteKeys(Vec<String>),
    Grok(Option<String>, Box<Grok>),
    ListToMap(Box<ListToMap>),
    Case(bool, Vec<String>),
    Trim(Vec<String>),
    ParseJson(Option<String>, Option<String>),
    KeyValue(Box<KeyValue>),
    Split(Vec<(String, String)>),
    Substitute(Vec<(String, Regex, String)>),
    Convert(Vec<(String, Kind)>),
    Vpc,
}

/// `InvalidParameterException` with `message`. (memory: the wording of every refusal here)
pub(crate) fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidParameterException", message)
}

/// The processors of a `transformerConfig`.
pub(crate) fn read(config: &Value) -> Result<Vec<Proc>, ApiError> {
    let list = config
        .as_array()
        .ok_or_else(|| invalid("transformerConfig must be a list of processors."))?;
    if list.is_empty() || list.len() > MAX_PROCESSORS {
        return Err(invalid(format!(
            "A transformer must have between 1 and {MAX_PROCESSORS} processors."
        )));
    }
    list.iter().enumerate().map(|(i, p)| one(i, p)).collect()
}

fn one(index: usize, p: &Value) -> Result<Proc, ApiError> {
    let obj = p
        .as_object()
        .ok_or_else(|| invalid("Each processor must be an object."))?;
    let (name, body) = match (obj.len(), obj.iter().next()) {
        (1, Some((k, v))) => (k.as_str(), v),
        _ => {
            return Err(invalid(
                "Each processor must set exactly one processor type.",
            ));
        }
    };
    let body = body
        .as_object()
        .ok_or_else(|| invalid(format!("The {name} processor must be an object.")))?;
    if index == 0 && !PARSERS.contains(&name) {
        // Recorded 2026-11-19.
        return Err(invalid(format!(
            "Transformer config should begin with parser. Allowed parsers are {}",
            PARSERS.join(", ")
        )));
    }
    if VENDED.contains(&name) {
        if index != 0 {
            return Err(invalid(format!(
                "The {name} processor must be the first processor in the transformer."
            )));
        }
        if opt(body, "source").is_some_and(|s| s != "@message") {
            return Err(invalid(format!(
                "The source of {name} can only be @message."
            )));
        }
    }
    build(name, body)
}

fn build(name: &str, b: &Map<String, Value>) -> Result<Proc, ApiError> {
    Ok(match name {
        "addKeys" => Proc::AddKeys(
            entries(b, "entries")?
                .iter()
                .map(|e| {
                    Ok((
                        need(e, "key")?,
                        need(e, "value")?,
                        flag(e, "overwriteIfExists"),
                    ))
                })
                .collect::<Result<_, ApiError>>()?,
        ),
        "copyValue" => Proc::CopyValue(moves(b, "source", "target")?),
        "moveKeys" => Proc::MoveKeys(moves(b, "source", "target")?),
        "renameKeys" => Proc::RenameKeys(moves(b, "key", "renameTo")?),
        "csv" => Proc::Csv(Box::new(csv(b)?)),
        "dateTimeConverter" => Proc::DateTime(Box::new(date_time(b)?)),
        "deleteKeys" => Proc::DeleteKeys(strings(b, "withKeys")?),
        "grok" => {
            let pattern = need(b, "match")?;
            let g = grok::compile(&pattern).map_err(invalid)?;
            Proc::Grok(opt(b, "source"), Box::new(g))
        }
        "listToMap" => Proc::ListToMap(Box::new(list_to_map(b)?)),
        "lowerCaseString" => Proc::Case(false, strings(b, "withKeys")?),
        "upperCaseString" => Proc::Case(true, strings(b, "withKeys")?),
        "trimString" => Proc::Trim(strings(b, "withKeys")?),
        "parseJSON" => Proc::ParseJson(opt(b, "source"), opt(b, "destination")),
        "parseWAF" => Proc::ParseJson(None, None),
        "parseVPC" => Proc::Vpc,
        "parseKeyValue" => Proc::KeyValue(Box::new(key_value(b))),
        "splitString" => Proc::Split(
            entries(b, "entries")?
                .iter()
                .map(|e| Ok((need(e, "source")?, need(e, "delimiter")?)))
                .collect::<Result<_, ApiError>>()?,
        ),
        "substituteString" => Proc::Substitute(substitutions(b)?),
        "typeConverter" => Proc::Convert(conversions(b)?),
        "parseCloudfront" | "parseRoute53" | "parsePostgres" | "parseToOCSF" => {
            return Err(invalid(format!(
                "The {name} processor is not run by doze-aws: the logs it parses are vended by AWS services that do not run locally."
            )));
        }
        other => return Err(invalid(format!("Unknown processor type: {other}."))),
    })
}

fn opt(b: &Map<String, Value>, key: &str) -> Option<String> {
    b.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn need(b: &Map<String, Value>, key: &str) -> Result<String, ApiError> {
    opt(b, key)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(format!("{key} is required.")))
}

fn flag(b: &Map<String, Value>, key: &str) -> bool {
    b.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn entries<'a>(
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

fn strings(b: &Map<String, Value>, key: &str) -> Result<Vec<String>, ApiError> {
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

fn moves(b: &Map<String, Value>, from: &str, to: &str) -> Result<Vec<Move>, ApiError> {
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

fn csv(b: &Map<String, Value>) -> Result<Csv, ApiError> {
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

fn zone(b: &Map<String, Value>, key: &str) -> Result<Zone, ApiError> {
    match opt(b, key) {
        None => Ok(Zone::utc()),
        Some(name) => Zone::named(&name)
            .ok_or_else(|| invalid(format!("{key} {name} is not a known time zone."))),
    }
}

fn date_time(b: &Map<String, Value>) -> Result<DateTime, ApiError> {
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

fn list_to_map(b: &Map<String, Value>) -> Result<ListToMap, ApiError> {
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

fn key_value(b: &Map<String, Value>) -> KeyValue {
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

fn substitutions(b: &Map<String, Value>) -> Result<Vec<(String, Regex, String)>, ApiError> {
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

fn conversions(b: &Map<String, Value>) -> Result<Vec<(String, Kind)>, ApiError> {
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
