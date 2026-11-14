//! `GetExecutionHistory`: the events of an execution, read from their rows in id order (or the reverse), a page at a time. With
//! `includeExecutionData: false` the payloads (`input`, `output`, `parameters`) are left out of the details.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::drive::history_key;
use crate::req::{Req, token};

impl Core {
    pub(crate) fn history_of(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let arn = self.execution(r.s("executionArn"))?.arn.clone();
        let size = r.page_size(100);
        let after = r.page_from()?;
        let reverse = r.flag("reverseOrder");
        let (from, to) = if reverse {
            let to = if after == 0 {
                format!("{arn}0")
            } else {
                history_key(&arn, after as u64)
            };
            (format!("{arn}/"), to)
        } else {
            (history_key(&arn, after as u64 + 1), format!("{arn}0"))
        };
        let rows = self
            .history
            .scan(self.db.as_ref(), &from, &to, size + 1, reverse)
            .map_err(|e| ApiError::internal(format!("could not read the history: {e}")))?;
        let data =
            r.v.get("includeExecutionData")
                .and_then(Value::as_bool)
                .unwrap_or(true);
        let mut events = Vec::with_capacity(size);
        let mut last = 0;
        for (_, bytes) in rows.iter().take(size) {
            let Ok(mut ev) = serde_json::from_slice::<Value>(bytes) else {
                continue;
            };
            last = ev.get("id").and_then(Value::as_u64).unwrap_or(0);
            if !data {
                strip(&mut ev);
            }
            events.push(ev);
        }
        let mut out = serde_json::Map::new();
        out.insert("events".into(), Value::Array(events));
        if rows.len() > size {
            out.insert(
                "nextToken".into(),
                json!(token(usize::try_from(last).unwrap_or(0))),
            );
        }
        Ok(Value::Object(out))
    }
}

fn strip(ev: &mut Value) {
    let Some(m) = ev.as_object_mut() else { return };
    for (k, v) in m.iter_mut() {
        if k.ends_with("EventDetails")
            && let Some(d) = v.as_object_mut()
        {
            for field in ["input", "output", "parameters"] {
                d.remove(field);
            }
        }
    }
}
