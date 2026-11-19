//! The fields of one event as a query sees them: the `@` fields CloudWatch Logs makes, the members of a JSON event flattened with
//! dots (`a.b.c`, an array's members as `a.0`), and the fields of a Lambda function's `START`, `END` and `REPORT` lines. (memory: the
//! field names are those of AWS's "Supported logs and discovered fields" page; the flattening and the Lambda units are as remembered:
//! `@memorySize` and `@maxMemoryUsed` in bytes, the durations in milliseconds.)

use serde_json::Value;

use super::value::Val;

/// The most fields one event is taken apart into (memory: Insights discovers up to 1,000 fields in an event).
const MAX_FIELDS: usize = 1000;
/// How deep a JSON event is flattened.
const MAX_DEPTH: usize = 16;

/// One event, borrowed from where it was read.
#[derive(Debug)]
pub(crate) struct Event<'a> {
    pub(crate) ts: i64,
    pub(crate) ingested: i64,
    pub(crate) message: &'a str,
    /// What the group's transformer made of the message, if it has one: the fields are read from it.
    pub(crate) transformed: Option<&'a str>,
    pub(crate) stream: &'a str,
    pub(crate) group: &'a str,
    pub(crate) account: &'a str,
    pub(crate) ptr: String,
    /// The group is a Lambda function's (`/aws/lambda/...`).
    pub(crate) lambda: bool,
}

/// A row: fields in the order they were made.
#[derive(Clone, Debug, Default)]
pub(crate) struct Row(pub(crate) Vec<(String, Val)>);

impl Row {
    pub(crate) fn get(&self, name: &str) -> Option<&Val> {
        self.0.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    /// Sets a field, replacing one of the same name.
    pub(crate) fn set(&mut self, name: &str, value: Val) {
        match self.0.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = value,
            None => self.0.push((name.to_owned(), value)),
        }
    }

    /// The row's time (`@timestamp`).
    pub(crate) fn ts(&self) -> Option<i64> {
        self.get("@timestamp").and_then(Val::time)
    }
}

/// The row an event is read as.
pub(crate) fn row(e: &Event<'_>) -> Row {
    let mut r = Row(Vec::with_capacity(8));
    r.0.push(("@timestamp".into(), Val::Time(e.ts)));
    r.0.push(("@message".into(), Val::Str(e.message.to_owned())));
    r.0.push(("@logStream".into(), Val::Str(e.stream.to_owned())));
    r.0.push((
        "@log".into(),
        Val::Str(format!("{}:{}", e.account, e.group)),
    ));
    r.0.push(("@ingestionTime".into(), Val::Time(e.ingested)));
    r.0.push(("@ptr".into(), Val::Str(e.ptr.clone())));
    let body = e.transformed.unwrap_or(e.message);
    if body.trim_start().starts_with('{')
        && let Ok(v) = serde_json::from_str::<Value>(body)
    {
        flatten("", &v, &mut r, 0);
    }
    if e.lambda {
        lambda_fields(e.message, &mut r);
    }
    r
}

/// Every field a JSON value holds, under `prefix`.
fn flatten(prefix: &str, v: &Value, out: &mut Row, depth: usize) {
    if out.0.len() >= MAX_FIELDS {
        return;
    }
    let key = |k: &str| {
        if prefix.is_empty() {
            k.to_owned()
        } else {
            format!("{prefix}.{k}")
        }
    };
    match v {
        Value::Object(m) if depth < MAX_DEPTH => {
            for (k, x) in m {
                flatten(&key(k), x, out, depth + 1);
            }
        }
        Value::Array(a) if depth < MAX_DEPTH => {
            for (i, x) in a.iter().enumerate() {
                flatten(&key(&i.to_string()), x, out, depth + 1);
            }
        }
        _ if prefix.is_empty() => {}
        Value::String(s) => out.set(prefix, Val::Str(s.clone())),
        Value::Number(n) => out.set(prefix, n.as_f64().map_or(Val::Null, Val::Num)),
        Value::Bool(b) => out.set(prefix, Val::Bool(*b)),
        Value::Null => {}
        other => out.set(prefix, Val::Str(other.to_string())),
    }
}

/// The fields of a Lambda platform line: `START RequestId: ...`, `END RequestId: ...`, `REPORT RequestId: ... Duration: 1.23 ms ...`.
fn lambda_fields(message: &str, out: &mut Row) {
    let kind = ["START", "END", "REPORT"]
        .into_iter()
        .find(|k| message.starts_with(&format!("{k} RequestId: ")));
    let Some(kind) = kind else {
        return;
    };
    out.set("@type", Val::Str(kind.to_owned()));
    let rest = message
        .get(kind.len() + " RequestId: ".len()..)
        .unwrap_or_default();
    let id: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
    out.set("@requestId", Val::Str(id));
    if kind != "REPORT" {
        return;
    }
    for part in message.split('\t') {
        let Some((label, value)) = part.trim().split_once(": ") else {
            continue;
        };
        let (number, unit) = value.split_once(' ').unwrap_or((value, ""));
        let Ok(n) = number.parse::<f64>() else {
            continue;
        };
        let name = match label {
            "Duration" => "@duration",
            "Billed Duration" => "@billedDuration",
            "Memory Size" => "@memorySize",
            "Max Memory Used" => "@maxMemoryUsed",
            "Init Duration" => "@initDuration",
            _ => continue,
        };
        let n = if unit.starts_with("MB") {
            n * 1_000_000.0
        } else {
            n
        };
        out.set(name, Val::Num(n));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(message: &str) -> Event<'_> {
        Event {
            ts: 1000,
            ingested: 1001,
            message,
            transformed: None,
            stream: "s",
            group: "/aws/lambda/f",
            account: "111122223333",
            ptr: "p".into(),
            lambda: true,
        }
    }

    #[test]
    fn json_is_flattened_and_lambda_reports_are_read() {
        let r = row(&event(r#"{"a":{"b":1},"list":["x","y"],"ok":true}"#));
        assert_eq!(r.get("a.b"), Some(&Val::Num(1.0)));
        assert_eq!(r.get("list.1"), Some(&Val::Str("y".into())));
        assert_eq!(r.get("ok"), Some(&Val::Bool(true)));
        assert_eq!(
            r.get("@log"),
            Some(&Val::Str("111122223333:/aws/lambda/f".into()))
        );
        let report = "REPORT RequestId: abc-1\tDuration: 12.50 ms\tBilled Duration: 13 ms\tMemory Size: 128 MB\tMax Memory Used: 64 MB\t";
        let r = row(&event(report));
        assert_eq!(r.get("@requestId"), Some(&Val::Str("abc-1".into())));
        assert_eq!(r.get("@duration"), Some(&Val::Num(12.5)));
        assert_eq!(r.get("@memorySize"), Some(&Val::Num(128_000_000.0)));
        assert_eq!(r.get("@type"), Some(&Val::Str("REPORT".into())));
    }
}
