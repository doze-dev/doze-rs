//! The Logs Insights read operations: `GetQueryResults`, `StopQuery`, `DescribeQueries`, `ListLogGroupsForQuery` and
//! `GetLogRecord`, with the shapes they answer in.

use std::sync::Arc;

use aws_wire::ApiError;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::core::Core;
use crate::insights::{self, record};
use crate::model::Event;
use crate::queries::{QueryRun, Status};
use crate::query_ops::{PAGE, no_query};
use crate::req::{Put, Req, backend_error, invalid};

impl Core {
    pub(crate) fn get_query_results(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let run = self
            .queries
            .get(r.s("queryId"), r.region(), r.account())
            .ok_or_else(|| no_query(r.s("queryId")))?;
        let size = r
            .int("maxItems")
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n > 0)
            .unwrap_or(PAGE);
        let from = match r.opt("nextToken") {
            Some(t) => t
                .parse::<usize>()
                .map_err(|_| invalid("The specified nextToken is invalid."))?,
            None => 0,
        };
        Ok(run.with_state(|s| {
            let page: Vec<Value> = s
                .rows
                .iter()
                .skip(from)
                .take(size)
                .map(|row| {
                    Value::Array(
                        row.iter()
                            .map(|(f, v)| json!({"field": f, "value": v}))
                            .collect(),
                    )
                })
                .collect();
            #[allow(clippy::cast_precision_loss)]
            let mut out = json!({
                "queryLanguage": "CWLI",
                "results": page,
                "statistics": {
                    "recordsMatched": s.matched as f64,
                    "recordsScanned": s.scanned as f64,
                    "estimatedRecordsSkipped": 0.0,
                    "bytesScanned": s.bytes as f64,
                    "estimatedBytesSkipped": 0.0,
                    "logGroupsScanned": run.groups.len() as f64,
                    "resultCount": s.rows.len() as f64,
                },
                "status": s.status.name(),
            });
            if from + size < s.rows.len() {
                out.put("nextToken", json!((from + size).to_string()));
            }
            out
        }))
    }

    pub(crate) fn stop_query(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let run = self
            .queries
            .get(r.s("queryId"), r.region(), r.account())
            .ok_or_else(|| no_query(r.s("queryId")))?;
        let ended = run.with_state(|s| {
            if s.status.live() {
                s.status = Status::Cancelled;
                None
            } else {
                Some(s.status.name())
            }
        });
        if let Some(status) = ended {
            // Recorded 2026-11-19.
            return Err(backend_error(
                "InvalidParameterException",
                &format!("Query is already ended with {status}"),
            ));
        }
        run.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(json!({"success": true}))
    }

    pub(crate) fn describe_queries(&self, r: &Req<'_>) -> Value {
        let (group, status) = (r.opt("logGroupName"), r.opt("status"));
        let all: Vec<Arc<QueryRun>> = self
            .queries
            .of(r.region(), r.account())
            .into_iter()
            .filter(|q| group.is_none_or(|g| q.groups.iter().any(|n| n == g)))
            .filter(|q| status.is_none_or(|s| q.status().name() == s))
            .filter(|_| r.opt("queryLanguage").is_none_or(|l| l == "CWLI"))
            .collect();
        let size = r
            .int("maxResults")
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(1000);
        let from = r
            .opt("nextToken")
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(0);
        let shown: Vec<Value> = all
            .iter()
            .skip(from)
            .take(size)
            .map(|q| query_info(q))
            .collect();
        let mut out = json!({"queries": shown});
        if from + size < all.len() {
            out.put("nextToken", json!((from + size).to_string()));
        }
        out
    }

    pub(crate) fn list_groups_for_query(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let run = self
            .queries
            .get(r.s("queryId"), r.region(), r.account())
            .ok_or_else(|| no_query(r.s("queryId")))?;
        let size = r
            .int("maxResults")
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(50);
        let from = r
            .opt("nextToken")
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(0);
        // Recorded 2026-11-19: the groups as the query named them (a name given as a name).
        let listed: Vec<&String> = run.listed.iter().skip(from).take(size).collect();
        let mut out = json!({"logGroupIdentifiers": listed});
        if from + size < run.listed.len() {
            out.put("nextToken", json!((from + size).to_string()));
        }
        Ok(out)
    }

    /// Every field of the event a `@ptr` names, `@timestamp` and `@ingestionTime` as milliseconds. (memory)
    pub(crate) fn get_log_record(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        // Recorded 2026-11-19.
        let bad = || invalid("InvalidParameter Unable to deserialize token");
        let (region, group, key) =
            insights::job::unpoint(r.s("logRecordPointer")).ok_or_else(bad)?;
        if region != r.region() {
            return Err(bad());
        }
        let gone = || {
            ApiError::modeled(
                "ResourceNotFoundException",
                "The specified log event does not exist.",
            )
        };
        let no = u32::from_str_radix(key.get(..8).unwrap_or_default(), 16).map_err(|_| bad())?;
        let (stream, stream_made) = self
            .streams
            .iter()
            .find(|(_, s)| s.no == no && s.group == group)
            .map(|(_, s)| (s.name.clone(), s.created_ms))
            .ok_or_else(gone)?;
        let group_made = self.groups.get(&group).map_or(0, |g| g.created_ms);
        let bytes = self
            .events
            .get(self.db.as_ref(), &key)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(gone)?;
        let e = Event::decode(&bytes).map_err(|_| gone())?;
        let ts = key
            .get(8..24)
            .and_then(|h| i64::from_str_radix(h, 16).ok())
            .unwrap_or(0);
        let seen = record::Event {
            ts,
            ingested: e.ingested_ms,
            message: &e.message,
            transformed: e.transformed.as_deref(),
            stream: &stream,
            group: &group,
            account: r.account(),
            region: r.region(),
            ptr: String::new(),
            lambda: group.starts_with("/aws/lambda/"),
        };
        let mut fields = serde_json::Map::new();
        for (k, v) in record::row(&seen).0 {
            let text = match (k.as_str(), &v) {
                ("@ptr", _) => continue,
                (_, insights::value::Val::Time(t)) => t.to_string(),
                _ => v.text(),
            };
            fields.insert(k, Value::String(text));
        }
        // Recorded 2026-11-19: the group's id, and the stream's as `<group id>::<hex SHA-256 of its name>::<made, ms>`.
        let id = group_id(r.region(), r.account(), &group, group_made);
        fields.insert(
            "@logStreamId".into(),
            Value::String(format!(
                "{id}::{}::{stream_made}",
                aws_core::hex::encode(&Sha256::digest(stream.as_bytes()))
            )),
        );
        fields.insert("@logGroupId".into(), Value::String(id));
        Ok(json!({"logRecord": fields}))
    }
}

/// A log group's id, as `@logGroupId` shows it: AWS mints its own; locally it is drawn from the group's ARN and the moment it was
/// made, so it stays the same for the group's life and a group made again under the same name has another.
fn group_id(region: &str, account: &str, name: &str, made_ms: i64) -> String {
    let d = Sha256::digest(
        format!(
            "{}\u{1}{made_ms}",
            crate::model::group_arn(region, account, name)
        )
        .as_bytes(),
    );
    let h = aws_core::hex::encode(d.get(..16).unwrap_or_default());
    let part = |a: usize, b: usize| h.get(a..b).unwrap_or_default();
    format!(
        "{}-{}-{}-{}-{}",
        part(0, 8),
        part(8, 12),
        part(12, 16),
        part(16, 20),
        part(20, 32)
    )
}

fn query_info(q: &QueryRun) -> Value {
    let mut v = json!({
        "queryLanguage": "CWLI",
        "queryId": q.id,
        "queryString": q.query,
        "status": q.status().name(),
        "createTime": q.created_ms,
    });
    if let Some(g) = q.groups.first() {
        v.put("logGroupName", json!(g));
    }
    q.with_state(|s| {
        if !s.status.live() {
            v.put("queryDuration", json!(s.duration_ms));
            #[allow(clippy::cast_precision_loss)]
            v.put("bytesScanned", json!(s.bytes as f64));
        }
    });
    v
}
