//! An event as EventBridge makes it from a `PutEvents` entry: the envelope AWS wraps around `Detail`.
//!
//! The delivered document is recorded (2026-11-09): `version` `"0"`, `id`, `detail-type`, `source`, `account`, `time`, `region`,
//! `resources`, `detail`. AWS writes them in that order; the members of `detail` itself come out in sorted order here.

use serde_json::Value;

use crate::util::iso;

/// One event on its way through a bus.
#[derive(Clone, Debug)]
pub(crate) struct Event {
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) detail_type: String,
    pub(crate) account: String,
    pub(crate) region: String,
    pub(crate) time_ms: i64,
    pub(crate) resources: Vec<String>,
    pub(crate) detail: Value,
    /// Set on an event a replay puts back: such an event is not archived again.
    pub(crate) replay_name: Option<String>,
}

impl Event {
    /// The event as the document rules are matched against, and as a target receives it, in AWS's order of members.
    pub(crate) fn text(&self) -> String {
        let s = |v: &str| Value::from(v).to_string();
        let resources = Value::from(self.resources.clone()).to_string();
        let mut out = format!(
            "{{\"version\":\"0\",\"id\":{},\"detail-type\":{},\"source\":{},\"account\":{},\"time\":{},\"region\":{},\"resources\":{},\"detail\":{}",
            s(&self.id),
            s(&self.detail_type),
            s(&self.source),
            s(&self.account),
            s(&iso(self.time_ms)),
            s(&self.region),
            resources,
            self.detail,
        );
        if let Some(name) = &self.replay_name {
            out.push_str(&format!(",\"replay-name\":{}", s(name)));
        }
        out.push('}');
        out
    }

    /// The same document as a value, for matching.
    pub(crate) fn doc(&self) -> Value {
        serde_json::from_str(&self.text()).unwrap_or(Value::Null)
    }

    /// Reads back what `text` wrote (an archived event).
    pub(crate) fn of_text(text: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(text).ok()?;
        let get = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        Some(Self {
            id: get("id")?,
            source: get("source")?,
            detail_type: get("detail-type")?,
            account: get("account")?,
            region: get("region")?,
            time_ms: aws_core::time::parse_rfc3339(&get("time")?)?.saturating_mul(1000),
            resources: v
                .get("resources")?
                .as_array()?
                .iter()
                .filter_map(|r| r.as_str().map(str::to_owned))
                .collect(),
            detail: v.get("detail")?.clone(),
            replay_name: get("replay-name"),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_event_is_written_in_awss_order_and_read_back() {
        let e = Event {
            id: "1".into(),
            source: "shop.orders".into(),
            detail_type: "placed".into(),
            account: "111122223333".into(),
            region: "us-east-1".into(),
            time_ms: 1_700_000_000_000,
            resources: vec!["arn:aws:x:::order/1".into()],
            detail: json!({"total": 250}),
            replay_name: None,
        };
        assert_eq!(
            e.text(),
            r#"{"version":"0","id":"1","detail-type":"placed","source":"shop.orders","account":"111122223333","time":"2023-11-14T22:13:20Z","region":"us-east-1","resources":["arn:aws:x:::order/1"],"detail":{"total":250}}"#
        );
        let back = Event::of_text(&e.text()).unwrap();
        assert_eq!(back.time_ms, e.time_ms);
        assert_eq!(back.detail, e.detail);
        assert_eq!(back.resources, e.resources);
    }
}
