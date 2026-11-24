//! A module's console and actions as JSON, for the snapshot each module pins (`console.snapshot.json`): the app's
//! `consoles` fixture (`apps/doze-app/fixtures/consoles.ts`) reads those files, so the consoles the app is drawn and
//! shot against are what the modules publish, never a hand copy. Only the fields that are set are written, in the
//! daemon's JSON names.

use super::console::{Action, ConsoleSpec, Node, Panel};
use serde_json::{Map, Value, json};

/// The spec and actions as `{"spec": …, "actions": […]}`.
#[must_use]
pub fn console_json(spec: &ConsoleSpec, actions: &[Action]) -> Value {
    let mut s = Map::new();
    if let Some(f) = &spec.front {
        s.insert("front".into(), json!(f));
    }
    if let Some(w) = &spec.web_url {
        s.insert("web_url".into(), json!(w));
    }
    s.insert(
        "panels".into(),
        Value::Array(spec.panels.iter().map(panel).collect()),
    );
    if !spec.tabs.is_empty() {
        let tabs = spec
            .tabs
            .iter()
            .map(|t| json!({"id": t.id, "title": t.title, "body": node(&t.body)}))
            .collect();
        s.insert("tabs".into(), Value::Array(tabs));
    }
    if !spec.extend.is_empty() {
        let extend = spec
            .extend
            .iter()
            .map(|e| json!({"after": e.after, "node": node(&e.node)}))
            .collect();
        s.insert("extend".into(), Value::Array(extend));
    }
    let actions: Vec<Value> = actions
        .iter()
        .map(|a| {
            let mut o = Map::new();
            o.insert("id".into(), json!(a.id));
            o.insert("label".into(), json!(a.label));
            o.insert("kind".into(), json!(a.kind));
            if a.destructive {
                o.insert("destructive".into(), json!(true));
            }
            if let Some(h) = &a.input_hint {
                o.insert("input_hint".into(), json!(h));
            }
            Value::Object(o)
        })
        .collect();
    json!({"spec": Value::Object(s), "actions": actions})
}

fn panel(p: &Panel) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), json!(p.id));
    o.insert("title".into(), json!(p.title));
    o.insert("kind".into(), json!(p.kind));
    if !p.source.is_empty() {
        o.insert("source".into(), json!(p.source));
    }
    let columns = p
        .columns
        .iter()
        .map(|c| {
            let mut col = Map::new();
            col.insert("key".into(), json!(c.key));
            col.insert("title".into(), json!(c.title));
            if !c.role.is_empty() {
                col.insert("role".into(), json!(c.role));
            }
            if !c.align.is_empty() {
                col.insert("align".into(), json!(c.align));
            }
            Value::Object(col)
        })
        .collect();
    o.insert("columns".into(), Value::Array(columns));
    if let Some(s) = &p.select {
        o.insert("select".into(), json!(s));
    }
    Value::Object(o)
}

fn node(n: &Node) -> Value {
    let mut o = Map::new();
    o.insert("kind".into(), json!(n.kind));
    for (k, v) in [
        ("id", &n.id),
        ("title", &n.title),
        ("panel", &n.panel),
        ("from", &n.from),
        ("text", &n.text),
    ] {
        if !v.is_empty() {
            o.insert(k.into(), json!(v));
        }
    }
    if n.weight != 0 {
        o.insert("weight".into(), json!(n.weight));
    }
    if !n.actions.is_empty() {
        o.insert("actions".into(), json!(n.actions));
    }
    if !n.children.is_empty() {
        o.insert(
            "children".into(),
            Value::Array(n.children.iter().map(node).collect()),
        );
    }
    Value::Object(o)
}

/// Pins `now` to the snapshot at `path`: rewrites it with `DOZE_UPDATE_SNAPSHOT=1`, else says how they differ.
/// Compared as values, not text: serde_json keeps keys in insertion order when another crate of the workspace turns
/// on its `preserve_order`, and in name order otherwise.
///
/// # Errors
///
/// When the snapshot cannot be written, or is missing or differs from `now`: the console changed, so the snapshot
/// and the app's fixture must be regenerated.
pub fn pin(path: &str, now: &Value) -> Result<(), String> {
    if std::env::var_os("DOZE_UPDATE_SNAPSHOT").is_some() {
        let text = serde_json::to_string_pretty(now).map_err(|e| e.to_string())? + "\n";
        std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?;
    }
    let pinned: Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    if &pinned == now {
        return Ok(());
    }
    Err(format!(
        "the console changed: review it, run DOZE_UPDATE_SNAPSHOT=1, then regenerate \
         apps/doze-app/fixtures/scripts/consoles.ndjson (bun run apps/doze-app/fixtures/consoles.ts)\n\
         pinned: {pinned}\nnow:    {now}"
    ))
}
