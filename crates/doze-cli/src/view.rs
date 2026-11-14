//! Rule 6, one model and three renderings: each command builds a typed result once, and it is drawn for a terminal
//! (colour), for a pipe (the same words without it) and as JSON (the envelope's `data`). Plain and JSON are the stable
//! contracts the transcript tests pin.

use crate::cmd::{docs, doctor, modules, verify, version};
use crate::paint::Paint;
use crate::plan::Plan;
use serde_json::{Value, json};

/// A command's result.
#[derive(Debug)]
pub enum View {
    /// Printed as it is (a completion script).
    Raw(String),
    /// Nothing to print (the background refresh).
    Silent,
    /// A page was opened in the browser.
    Opened(String),
    Version(version::Info),
    Plan(Plan),
    Docs(Box<docs::Page>),
    Modules(modules::Listing),
    Verify(verify::Report),
    Doctor(doctor::Report),
}

impl View {
    /// For a person: `p` says whether stdout takes colour.
    pub(crate) fn human(&self, p: Paint) -> String {
        match self {
            View::Raw(s) => s.clone(),
            View::Silent => String::new(),
            View::Opened(url) => format!("→ opened {url} in your browser\n"),
            View::Version(v) => v.human(),
            View::Plan(plan) => plan.render(p),
            View::Docs(page) => page.human(p),
            View::Modules(l) => l.human(p),
            View::Verify(r) => r.human(p),
            View::Doctor(r) => r.human(p),
        }
    }

    /// The envelope's `data`.
    pub(crate) fn json(&self) -> Value {
        let v = match self {
            View::Raw(_) | View::Silent => Ok(Value::Null),
            View::Opened(url) => Ok(json!({ "opened": url })),
            View::Version(v) => serde_json::to_value(v),
            View::Plan(plan) => serde_json::to_value(&plan.groups)
                .map(|groups| json!({ "dry_run": true, "applies_with": plan.applies_with, "plan": groups })),
            View::Docs(page) => serde_json::to_value(page),
            View::Modules(l) => serde_json::to_value(l),
            View::Verify(r) => serde_json::to_value(r),
            View::Doctor(r) => serde_json::to_value(r),
        };
        sorted(v.unwrap_or(Value::Null))
    }
}

/// The JSON contract has its object keys in sorted order, whatever the build. A `Value`'s map keeps insertion order
/// when any crate in the build turns on serde_json's `preserve_order` (cargo unifies features across a workspace
/// build), and sorts otherwise; so the order is fixed here rather than left to the feature set.
fn sorted(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut pairs: Vec<(String, Value)> = m.into_iter().collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(pairs.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::sorted;
    use serde_json::{Map, Value, json};

    #[test]
    fn keys_come_out_sorted_whatever_order_they_went_in() {
        let mut inner = Map::new();
        inner.insert("state".into(), json!("ok"));
        inner.insert("name".into(), json!("platform"));
        let mut outer = Map::new();
        outer.insert("z".into(), json!([Value::Object(inner)]));
        outer.insert("a".into(), json!(1));
        let text = serde_json::to_string(&sorted(Value::Object(outer))).unwrap_or_default();
        assert_eq!(text, r#"{"a":1,"z":[{"name":"platform","state":"ok"}]}"#);
    }
}
