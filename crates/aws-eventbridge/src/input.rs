//! What a target receives: the event as it is, a constant (`Input`), a part of it (`InputPath`), or a text made from it
//! (`InputTransformer`).
//!
//! A transformer's template is filled by replacing each `<name>` with the value its `InputPathsMap` path finds in the event, or
//! one of the predefined names. Recorded (2026-11-09): `{"message": "<who> did <kind>"}` with two paths becomes
//! `{"message": "ada did placed"}`. Everything else here is from the documentation and from memory: inside a JSON string a text value
//! goes in as it is and any other value as JSON; outside a string (in a template that is JSON) text is quoted and other values are
//! JSON; in a template that is plain text every text value goes in as it is; a path that finds nothing is an empty text inside a
//! string and `null` outside one.

use serde_json::Value;

use crate::jsonpath;

/// The names that need no path.
const PREDEFINED: [&str; 5] = [
    "aws.events.rule-arn",
    "aws.events.rule-name",
    "aws.events.event.ingestion-time",
    "aws.events.event",
    "aws.events.event.json",
];

/// What the predefined names stand for.
#[derive(Debug)]
pub(crate) struct Names<'a> {
    pub(crate) rule_arn: &'a str,
    pub(crate) rule_name: &'a str,
    pub(crate) ingestion_time: &'a str,
}

/// Why a target is refused at `PutTargets`: the whole call is refused with these words (recorded 2026-11-15; not a `FailedEntries`
/// entry).
pub(crate) fn check_target(target: &Value) -> Result<(), String> {
    let id = target.get("Id").and_then(Value::as_str).unwrap_or_default();
    let shapes = ["Input", "InputPath", "InputTransformer"]
        .iter()
        .filter(|m| target.get(**m).is_some())
        .count();
    if shapes > 1 {
        return Err(format!(
            "Only one of Input, InputPath, or InputTransformer must be provided for target {id}."
        ));
    }
    if let Some(input) = target.get("Input").and_then(Value::as_str)
        && serde_json::from_str::<Value>(input).is_err()
    {
        return Err("Parameter Input is not valid.".into());
    }
    if let Some(path) = target.get("InputPath").and_then(Value::as_str)
        && !jsonpath::is_valid(path)
    {
        return Err("Parameter InputPath is not valid.".into());
    }
    match target.get("InputTransformer") {
        Some(t) => check_transformer(id, t),
        None => Ok(()),
    }
}

fn check_transformer(id: &str, t: &Value) -> Result<(), String> {
    let paths = t.get("InputPathsMap").and_then(Value::as_object);
    for (name, path) in paths.into_iter().flatten() {
        if !path.as_str().is_some_and(jsonpath::is_valid) {
            return Err(format!("InputPathsMap path for {name} is not valid."));
        }
    }
    let template = t
        .get("InputTemplate")
        .and_then(Value::as_str)
        .unwrap_or_default();
    for name in placeholders(template) {
        let defined = paths.is_some_and(|p| p.contains_key(name)) || PREDEFINED.contains(&name);
        if !defined {
            return Err(format!(
                "InputTemplate for target {id} contains invalid placeholder {name}."
            ));
        }
    }
    json_once_filled(id, template)
}

/// A template must be JSON once every placeholder is `null`: a text template is a JSON string (`"hello <who>"`, quotes included).
/// Recorded 2026-11-15 for `hello <who>`: `Invalid InputTemplate for target queue : [Source: (String)"hello null"; line: 1,
/// column: 1].` The line and column of other faults are where the parser stopped `(memory)`.
fn json_once_filled(id: &str, template: &str) -> Result<(), String> {
    let mut filled = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('<') {
        let after = rest.get(start + 1..).unwrap_or_default();
        match after.find('>') {
            Some(end) if is_name(after.get(..end).unwrap_or_default()) => {
                filled.push_str(rest.get(..start).unwrap_or_default());
                filled.push_str("null");
                rest = after.get(end + 1..).unwrap_or_default();
            }
            _ => {
                filled.push_str(rest.get(..=start).unwrap_or_default());
                rest = after;
            }
        }
    }
    filled.push_str(rest);
    match serde_json::from_str::<Value>(&filled) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!(
            "Invalid InputTemplate for target {id} : [Source: (String)\"{filled}\"; line: {}, column: {}].",
            e.line(),
            e.column()
        )),
    }
}

/// The names between `<` and `>` in a template.
fn placeholders(template: &str) -> impl Iterator<Item = &str> {
    let mut rest = template;
    std::iter::from_fn(move || {
        loop {
            let start = rest.find('<')?;
            let after = rest.get(start + 1..)?;
            match after.find('>') {
                Some(end) if is_name(after.get(..end)?) => {
                    rest = after.get(end + 1..)?;
                    return after.get(..end);
                }
                _ => rest = after,
            }
        }
    })
}

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// The text a target is sent, or `None` when the event has nothing to send it (an `InputPath` that finds nothing).
pub(crate) fn shape(
    target: &Value,
    event: &Value,
    event_text: &str,
    names: &Names<'_>,
) -> Option<String> {
    if let Some(input) = target.get("Input").and_then(Value::as_str) {
        return Some(input.to_owned());
    }
    if let Some(path) = target.get("InputPath").and_then(Value::as_str) {
        return jsonpath::select(event, path).map(|v| v.to_string());
    }
    match target.get("InputTransformer") {
        Some(t) => Some(transform(t, event, event_text, names)),
        None => Some(event_text.to_owned()),
    }
}

fn transform(t: &Value, event: &Value, event_text: &str, names: &Names<'_>) -> String {
    let template = t
        .get("InputTemplate")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let paths = t.get("InputPathsMap").and_then(Value::as_object);
    let value_of = |name: &str| -> Option<Value> {
        match name {
            "aws.events.rule-arn" => Some(Value::from(names.rule_arn)),
            "aws.events.rule-name" => Some(Value::from(names.rule_name)),
            "aws.events.event.ingestion-time" => Some(Value::from(names.ingestion_time)),
            "aws.events.event" => Some(Value::from(event_text)),
            "aws.events.event.json" => Some(event.clone()),
            other => paths
                .and_then(|p| p.get(other))
                .and_then(Value::as_str)
                .and_then(|path| jsonpath::select(event, path)),
        }
    };
    let json_mode = matches!(template.trim_start().chars().next(), Some('{' | '['));
    let mut out = String::with_capacity(template.len());
    let mut in_string = false;
    let mut chars = template.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' if in_string => {
                out.push(c);
                if let Some((_, e)) = chars.next() {
                    out.push(e);
                }
            }
            '"' => {
                in_string = !in_string;
                out.push(c);
            }
            '<' => {
                let name = template
                    .get(i + 1..)
                    .and_then(|r| r.split_once('>'))
                    .map(|(n, _)| n);
                match name.filter(|n| is_name(n)) {
                    Some(n) => {
                        out.push_str(&fill(value_of(n), in_string || !json_mode));
                        // The name and the closing bracket are consumed.
                        for _ in 0..n.chars().count() + 1 {
                            chars.next();
                        }
                    }
                    None => out.push(c),
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// A value as it is written into the template: `raw` is a place where text goes in as it is.
fn fill(value: Option<Value>, raw: bool) -> String {
    match value {
        Some(Value::String(s)) if raw => s,
        Some(v) => v.to_string(),
        None if raw => String::new(),
        None => "null".to_owned(),
    }
}

#[cfg(test)]
mod tests;
