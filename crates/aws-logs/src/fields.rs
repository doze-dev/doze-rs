//! `GetLogGroupFields`: the fields of a group's events in a window, and the share of events that hold each.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::insights::record::{self, Event as Seen};
use crate::model::stream_key;
use crate::req::{Req, invalid, no_group};

/// Events read, newest first across the window (a local bound; AWS samples too).
const SCAN: usize = 10_000;
/// The model: with `time`, "the 8 minutes before and 8 minutes after this time are searched"; without it "the most recent 15
/// minutes".
const AROUND_MS: i64 = 8 * 60_000;
const RECENT_MS: i64 = 15 * 60_000;

impl Core {
    pub(crate) fn get_log_group_fields(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        if r.opt("logGroupName").is_some() && r.opt("logGroupIdentifier").is_some() {
            // The model: "You must include either logGroupIdentifier or logGroupName, but not both." Wording (memory).
            return Err(invalid(
                "LogGroup name and LogGroup identifier are mutually exclusive parameters.",
            ));
        }
        let group = self.group(r)?.name.clone();
        if self.groups.get(&group).is_none() {
            return Err(no_group());
        }
        let (from, to) = match r.int("time") {
            Some(t) => (t * 1000 - AROUND_MS, t * 1000 + AROUND_MS),
            None => (self.now_ms() - RECENT_MS, self.now_ms()),
        };
        let range = (
            format!("{:016x}", from.max(0)),
            format!("{:016x}", to.max(0) + 1),
        );
        let streams: Vec<(u32, String)> = self
            .streams
            .with_prefix(&stream_key(&group, ""))
            .map(|(_, s)| (s.no, s.name.clone()))
            .collect();
        let (mut total, mut counts) = (0u64, BTreeMap::<String, u64>::new());
        for (no, name) in streams {
            let Ok(rows) = self.rows(no, (&range.0, &range.1), SCAN, true, &|_| true) else {
                continue;
            };
            for row in rows {
                let e = Seen {
                    ts: row.ts,
                    ingested: row.event.ingested_ms,
                    message: &row.event.message,
                    transformed: row.event.transformed.as_deref(),
                    stream: &name,
                    group: &group,
                    account: r.account(),
                    ptr: String::new(),
                    lambda: group.starts_with("/aws/lambda/"),
                };
                total += 1;
                for (field, _) in record::row(&e).0 {
                    if field != "@ptr" {
                        *counts.entry(field).or_default() += 1;
                    }
                }
            }
        }
        let mut fields: Vec<(String, u64)> = counts
            .into_iter()
            .map(|(f, n)| (f, (n * 100 + total / 2) / total.max(1)))
            .collect();
        fields.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(json!({
            "logGroupFields": fields.iter().map(|(f, p)| json!({"name": f, "percent": p})).collect::<Vec<_>>(),
        }))
    }
}
