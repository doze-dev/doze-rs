//! Running one processor over an event's JSON object. A processor that cannot apply (a parser on text it cannot parse, a missing key,
//! a value of the wrong type) leaves the event as it is. (memory)

use serde_json::{Map, Number, Value};

use super::Meta;
use super::datetime;
use super::grok::Grok;
use super::path;
use super::procs::{Csv, DateTime, KeyValue, Kind, ListToMap, Move, Proc};

/// The fields of a version 2 VPC flow log record, in order. (memory)
const VPC: [&str; 14] = [
    "version",
    "account_id",
    "interface_id",
    "srcaddr",
    "dstaddr",
    "srcport",
    "dstport",
    "protocol",
    "packets",
    "bytes",
    "start",
    "end",
    "action",
    "log_status",
];

/// The event a processor works on.
pub(crate) struct Event<'a> {
    pub(crate) message: &'a str,
    pub(crate) meta: &'a Meta<'a>,
    pub(crate) obj: Map<String, Value>,
    /// Why a processor could not do its job (the first one that could not); the processors after it do not run.
    pub(crate) failed: Option<String>,
}

impl Event<'_> {
    /// The text a parser reads: the raw message for no source or `@message`, else the string at the path.
    fn text(&self, source: Option<&str>) -> Option<String> {
        match source {
            None | Some("@message") => Some(self.message.to_owned()),
            Some(p) => path::get(&self.obj, p)
                .and_then(Value::as_str)
                .map(str::to_owned),
        }
    }

    /// A value to copy: the metadata keys, or the value at the path.
    fn value(&self, source: &str) -> Option<Value> {
        let meta = match source {
            "@logGroupName" => Some(self.meta.group),
            "@logGroupStream" | "@logStream" => Some(self.meta.stream),
            "@accountId" => Some(self.meta.account),
            "@regionName" => Some(self.meta.region),
            "@message" => Some(self.message),
            _ => None,
        };
        meta.map(|m| Value::String(m.to_owned()))
            .or_else(|| path::get(&self.obj, source).cloned())
    }

    /// A parser's fields: under `destination`, or merged into the root.
    fn write(&mut self, destination: Option<&str>, fields: Map<String, Value>, overwrite: bool) {
        match destination {
            Some(d) => path::put(&mut self.obj, d, Value::Object(fields), overwrite),
            None => {
                for (k, v) in fields {
                    if overwrite || !self.obj.contains_key(&k) {
                        self.obj.insert(k, v);
                    }
                }
            }
        }
    }
}

/// Runs `p` over `e`.
pub(crate) fn run(p: &Proc, e: &mut Event<'_>) {
    match p {
        Proc::AddKeys(entries) => {
            for (k, v, overwrite) in entries {
                path::put(&mut e.obj, k, Value::String(v.clone()), *overwrite);
            }
        }
        Proc::CopyValue(moves) => copy(e, moves, false),
        Proc::MoveKeys(moves) => copy(e, moves, true),
        Proc::RenameKeys(moves) => rename(e, moves),
        Proc::Csv(c) => csv(e, c),
        Proc::DateTime(d) => date_time(e, d),
        Proc::DeleteKeys(keys) => keys.iter().for_each(|k| drop(path::take(&mut e.obj, k))),
        Proc::Grok(source, g) => grok(e, source.as_deref(), g),
        Proc::ListToMap(l) => list_to_map(e, l),
        Proc::Case(upper, keys) => strings(e, keys, |s| {
            if *upper {
                s.to_uppercase()
            } else {
                s.to_lowercase()
            }
        }),
        Proc::Trim(keys) => strings(e, keys, |s| s.trim().to_owned()),
        Proc::ParseJson(source, destination) => {
            let parsed = e
                .text(source.as_deref())
                .and_then(|t| serde_json::from_str::<Value>(&t).ok());
            if let Some(Value::Object(m)) = parsed {
                e.write(destination.as_deref(), m, true);
            }
        }
        Proc::KeyValue(kv) => key_value(e, kv),
        Proc::Split(entries) => {
            for (source, delimiter) in entries {
                if let Some(v) = path::get_mut(&mut e.obj, source)
                    && let Some(s) = v.as_str()
                {
                    *v = Value::Array(
                        s.split(delimiter.as_str())
                            .map(|x| Value::String(x.to_owned()))
                            .collect(),
                    );
                }
            }
        }
        Proc::Substitute(entries) => {
            for (source, re, to) in entries {
                if let Some(v) = path::get_mut(&mut e.obj, source)
                    && let Some(s) = v.as_str()
                {
                    *v = Value::String(re.replace_all(s, to.as_str()).into_owned());
                }
            }
        }
        Proc::Convert(entries) => {
            for (key, kind) in entries {
                if let Some(v) = path::get_mut(&mut e.obj, key)
                    && let Some(new) = convert(v, *kind)
                {
                    *v = new;
                }
            }
        }
        Proc::Vpc => vpc(e),
    }
}

fn grok(e: &mut Event<'_>, source: Option<&str>, g: &Grok) {
    let Some(text) = e.text(source) else {
        return;
    };
    let Some(caps) = g.re.captures(&text) else {
        return;
    };
    for (group, field) in &g.fields {
        if let Some(m) = caps.name(group) {
            path::set(&mut e.obj, field, Value::String(m.as_str().to_owned()));
        }
    }
}

fn copy(e: &mut Event<'_>, moves: &[Move], remove: bool) {
    for m in moves {
        // Recorded 2026-11-19 (moveKeys): a target under a key the event does not have is refused, not made.
        if remove
            && let Some((parent, _)) = m.target.rsplit_once('.')
            && !path::has(&e.obj, parent)
        {
            e.failed = Some(format!(
                "No corresponding field. Field Name: '{}', Processor: 'MoveKeysProcessor'",
                m.target
            ));
            return;
        }
        if !m.overwrite && path::has(&e.obj, &m.target) {
            continue;
        }
        let value = if remove {
            path::take(&mut e.obj, &m.source)
        } else {
            e.value(&m.source)
        };
        if let Some(v) = value {
            path::set(&mut e.obj, &m.target, v);
        }
    }
}

/// `renameKeys`: the new name is a key beside the old one. (memory)
fn rename(e: &mut Event<'_>, moves: &[Move]) {
    for m in moves {
        let target = match m.source.rsplit_once('.') {
            Some((parent, _)) => format!("{parent}.{}", m.target),
            None => m.target.clone(),
        };
        if (!m.overwrite && path::has(&e.obj, &target)) || target == m.source {
            continue;
        }
        if let Some(v) = path::take(&mut e.obj, &m.source) {
            path::set(&mut e.obj, &target, v);
        }
    }
}

fn strings(e: &mut Event<'_>, keys: &[String], f: impl Fn(&str) -> String) {
    for k in keys {
        if let Some(v) = path::get_mut(&mut e.obj, k)
            && let Some(s) = v.as_str()
        {
            *v = Value::String(f(s));
        }
    }
}

fn convert(v: &Value, kind: Kind) -> Option<Value> {
    let text = match v {
        Value::String(s) => s.trim().to_owned(),
        Value::Number(_) | Value::Bool(_) => v.to_string(),
        _ => return None,
    };
    Some(match kind {
        Kind::Text => Value::String(match v {
            Value::String(s) => s.clone(),
            _ => text,
        }),
        Kind::Integer => Value::Number(Number::from(text.parse::<i64>().ok()?)),
        Kind::Double => Value::Number(Number::from_f64(text.parse::<f64>().ok()?)?),
        Kind::Boolean => Value::Bool(match text.to_ascii_lowercase().as_str() {
            "true" => true,
            "false" => false,
            _ => return None,
        }),
    })
}

fn csv(e: &mut Event<'_>, c: &Csv) {
    let Some(text) = e.text(c.source.as_deref()) else {
        return;
    };
    let mut fields = Map::new();
    for (i, value) in split_csv(&text, c.quote, &c.delimiter)
        .into_iter()
        .enumerate()
    {
        let name = c
            .columns
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("column_{}", i + 1));
        fields.insert(name, Value::String(value));
    }
    e.write(c.destination.as_deref(), fields, true);
}

/// The values of one CSV line; a quoted value may hold the delimiter and a doubled quote.
fn split_csv(text: &str, quote: char, delimiter: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut rest) = (false, text);
    while let Some(c) = rest.chars().next() {
        if quoted {
            if c == quote {
                if rest
                    .get(c.len_utf8()..)
                    .is_some_and(|r| r.starts_with(quote))
                {
                    cur.push(quote);
                    rest = rest.get(2 * c.len_utf8()..).unwrap_or_default();
                    continue;
                }
                quoted = false;
            } else {
                cur.push(c);
            }
        } else if c == quote && cur.is_empty() {
            quoted = true;
        } else if let Some(after) = rest.strip_prefix(delimiter) {
            out.push(std::mem::take(&mut cur));
            rest = after;
            continue;
        } else {
            cur.push(c);
        }
        rest = rest.get(c.len_utf8()..).unwrap_or_default();
    }
    out.push(cur);
    out
}

fn date_time(e: &mut Event<'_>, d: &DateTime) {
    let Some(text) = path::get(&e.obj, &d.source)
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    let instant = d
        .patterns
        .iter()
        .find_map(|p| datetime::parse(p, &text, &d.source_zone));
    if let Some(ms) = instant {
        let out = datetime::format(&d.target_format, ms, &d.target_zone);
        path::set(&mut e.obj, &d.target, Value::String(out));
    }
}

fn list_to_map(e: &mut Event<'_>, l: &ListToMap) {
    let Some(list) = path::get(&e.obj, &l.source).and_then(Value::as_array) else {
        return;
    };
    let mut map = Map::new();
    for item in list {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let Some(key) = obj
            .get(&l.key)
            .map(|k| k.as_str().map_or_else(|| k.to_string(), str::to_owned))
        else {
            continue;
        };
        let value = match &l.value_key {
            Some(v) => match obj.get(v) {
                Some(x) => x.clone(),
                None => continue,
            },
            None => item.clone(),
        };
        match l.flatten {
            Some(true) if map.contains_key(&key) => {}
            Some(_) => {
                map.insert(key, value);
            }
            None => {
                if let Value::Array(a) = map.entry(key).or_insert_with(|| Value::Array(Vec::new()))
                {
                    a.push(value);
                }
            }
        }
    }
    match &l.target {
        Some(t) => drop(path::set(&mut e.obj, t, Value::Object(map))),
        None => e.write(None, map, true),
    }
}

fn key_value(e: &mut Event<'_>, kv: &KeyValue) {
    let Some(text) = e.text(kv.source.as_deref()) else {
        return;
    };
    let mut fields = Map::new();
    for pair in text
        .split(kv.field_delimiter.as_str())
        .filter(|p| !p.is_empty())
    {
        let (k, v) = match pair.split_once(kv.kv_delimiter.as_str()) {
            Some((k, v)) => (k, v.to_owned()),
            None => match &kv.non_match {
                Some(n) => (pair, n.clone()),
                None => continue,
            },
        };
        fields.insert(format!("{}{k}", kv.prefix), Value::String(v));
    }
    e.write(kv.destination.as_deref(), fields, kv.overwrite);
}

fn vpc(e: &mut Event<'_>) {
    let values: Vec<&str> = e.message.split_whitespace().collect();
    if values.len() != VPC.len() {
        return;
    }
    let mut fields = Map::new();
    for (name, value) in VPC.iter().zip(values) {
        let v = match value.parse::<i64>() {
            Ok(n) if *name != "account_id" => Value::Number(Number::from(n)),
            _ => Value::String(value.to_owned()),
        };
        fields.insert((*name).to_owned(), v);
    }
    e.write(None, fields, true);
}
