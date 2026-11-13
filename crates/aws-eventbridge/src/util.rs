//! Small helpers for building answers.

use serde_json::{Map, Value};

/// A timestamp as the wire carries it: seconds since the epoch, with milliseconds.
pub(crate) fn secs(ms: i64) -> Value {
    #[allow(clippy::cast_precision_loss)] // WHY: a time in ms is far below 2^53.
    Value::from(ms as f64 / 1000.0)
}

/// The time as `2026-11-13T10:15:00Z`.
pub(crate) fn iso(ms: i64) -> String {
    // The shared formatter writes `.000Z`; an event's time has no fraction.
    aws_core::time::format_iso8601(ms.div_euclid(1000)).replace(".000Z", "Z")
}

/// An answer under construction: members that are absent are left out.
#[derive(Debug, Default)]
pub(crate) struct Out(Map<String, Value>);

impl Out {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Sets a member.
    pub(crate) fn with(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.0.insert(name.to_owned(), value.into());
        self
    }

    /// Sets a member when there is a value for it.
    pub(crate) fn maybe<V: Into<Value>>(self, name: &str, value: Option<V>) -> Self {
        match value {
            Some(v) => self.with(name, v),
            None => self,
        }
    }

    /// Sets a member to JSON that was stored as text, when there is some.
    pub(crate) fn json(self, name: &str, text: Option<&String>) -> Self {
        match text.and_then(|t| serde_json::from_str::<Value>(t).ok()) {
            Some(v) => self.with(name, v),
            None => self,
        }
    }

    pub(crate) fn done(self) -> Value {
        Value::Object(self.0)
    }
}

/// `[{"Key": .., "Value": ..}]` from a map of tags.
pub(crate) fn tag_list(tags: &std::collections::BTreeMap<String, String>) -> Value {
    Value::Array(
        tags.iter()
            .map(|(k, v)| serde_json::json!({"Key": k, "Value": v}))
            .collect(),
    )
}
