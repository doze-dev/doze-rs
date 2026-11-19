//! `ImportApiKeys`: API keys from a CSV file, in AWS's documented format (developer guide, "API Gateway API key file format"): a
//! header row naming `Name` and `Key` (case ignored, any order), optionally `Description`, `Enabled` and `UsagePlanIds` (a quoted,
//! comma-separated list); other columns are ignored. A key is 20 to 128 characters, a name at most 1024. Missing values are an empty
//! string, and `Enabled` is `true`. A key whose value is already there is overwritten (two keys are the same key when their values
//! are). A row that cannot be imported is a warning; with `failonwarnings` nothing is imported. The warning texts are ours.

use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::doc::{Doc, put, text};
use crate::errors::bad;
use crate::req::{Req, Res, new_id};

/// The fields of one CSV line: commas separate, a field in double or single quotes may hold commas, and a doubled quote inside is one.
fn fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                if chars.peek() == Some(&q) {
                    cur.push(q);
                    chars.next();
                } else {
                    quote = None;
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') if cur.trim().is_empty() => {
                cur.clear();
                quote = Some(c);
            }
            (None, ',') => out.push(std::mem::take(&mut cur).trim().to_owned()),
            (None, c) => cur.push(c),
        }
    }
    out.push(cur.trim().to_owned());
    out
}

/// One row read, by lower-cased column name.
struct Row {
    name: String,
    key: String,
    description: String,
    enabled: bool,
    plans: Vec<String>,
}

fn row(header: &[String], line: &str) -> Row {
    let cells = fields(line);
    let get = |col: &str| {
        header
            .iter()
            .position(|h| h == col)
            .and_then(|i| cells.get(i))
            .cloned()
            .unwrap_or_default()
    };
    let enabled = get("enabled");
    Row {
        name: get("name"),
        key: get("key"),
        description: get("description"),
        enabled: !enabled.eq_ignore_ascii_case("false"),
        plans: get("usageplanids")
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

/// Why a row cannot be imported, if it cannot.
fn problem(core: &Core, n: usize, r: &Row) -> Option<String> {
    if !(20..=128).contains(&r.key.chars().count()) {
        return Some(format!("Row {n}: the key must be 20 to 128 characters"));
    }
    if r.name.chars().count() > 1024 {
        return Some(format!("Row {n}: the name must be at most 1024 characters"));
    }
    r.plans
        .iter()
        .find(|p| core.plans.get(p).is_none())
        .map(|p| format!("Row {n}: no usage plan has the id {p}"))
}

fn store(core: &mut Core, r: &Row, now: f64) -> String {
    let existing = core
        .keys
        .iter()
        .find(|(_, k)| text(&k.0, "value") == Some(r.key.as_str()))
        .map(|(id, k)| (id.to_owned(), k.0.clone()));
    let (id, mut k) = existing.unwrap_or_else(|| {
        let id = new_id(10);
        let k = json!({"id": id, "value": r.key, "createdDate": now, "stageKeys": []});
        (id, k)
    });
    put(&mut k, "name", json!(r.name));
    put(&mut k, "description", json!(r.description));
    put(&mut k, "enabled", json!(r.enabled));
    put(&mut k, "lastUpdatedDate", json!(now));
    core.keys.insert(id.clone(), Doc(k));
    for p in &r.plans {
        if let Some(Doc(plan)) = core.plans.get_mut(p)
            && let Some(Value::Array(keys)) = plan
                .as_object_mut()
                .map(|o| o.entry("_keys").or_insert_with(|| json!([])))
            && !keys.iter().any(|k| k.as_str() == Some(id.as_str()))
        {
            keys.push(json!(id));
        }
    }
    id
}

/// `ImportApiKeys`.
pub(super) fn import(core: &mut Core, r: &Req) -> Res {
    let raw = aws_core::base64::decode(r.s("body")).unwrap_or_default();
    let text = String::from_utf8_lossy(&raw);
    let mut lines = text
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty());
    let header: Vec<String> = lines
        .next()
        .map(fields)
        .unwrap_or_default()
        .iter()
        .map(|h| h.to_ascii_lowercase())
        .collect();
    if !header.iter().any(|h| h == "key") || !header.iter().any(|h| h == "name") {
        return Err(bad(
            "Invalid API key file: the header row must name the Name and Key columns",
        ));
    }
    let rows: Vec<Row> = lines.map(|l| row(&header, l)).collect();
    let warnings: Vec<String> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, x)| problem(core, i + 2, x))
        .collect();
    if r.flag("failOnWarnings") == Some(true) && !warnings.is_empty() {
        return Err(bad(format!(
            "API keys were not imported: {}",
            warnings.join("; ")
        )));
    }
    let now = r.now_secs();
    let ids: Vec<Value> = rows
        .iter()
        .enumerate()
        .filter(|(i, x)| problem(core, i + 2, x).is_none())
        .map(|(_, x)| x)
        .collect::<Vec<_>>()
        .into_iter()
        .map(|x| json!(store(core, x, now)))
        .collect();
    let mut out = Map::new();
    out.insert("ids".into(), Value::Array(ids));
    if !warnings.is_empty() {
        out.insert("warnings".into(), json!(warnings));
    }
    Ok(json!(out).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_fields_keep_their_commas() {
        assert_eq!(
            fields("true,My Key,apikey1234abcdefghij0123456789,\"c7y23b,glvrsr\""),
            vec![
                "true",
                "My Key",
                "apikey1234abcdefghij0123456789",
                "c7y23b,glvrsr"
            ]
        );
        assert_eq!(
            fields("'a,b', \"say \"\"hi\"\"\""),
            vec!["a,b", "say \"hi\""]
        );
    }
}
