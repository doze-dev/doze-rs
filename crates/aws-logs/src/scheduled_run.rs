//! Running scheduled queries: when each is next due, the region's tick, one run, and the delivery of its answer to S3.

use std::sync::Arc;

use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::doc_key;
use crate::queries::{Delivery, QueryRun};
use crate::query_ops::{Deliver, Plan};
use crate::req::{Put, name_of_identifier};
use crate::scheduled::{MAX_HISTORY, SCHEDULED, epoch_ms};

impl Core {
    /// When the next scheduled query is due, and the instant each enabled one is next due after `after`.
    fn due_after(v: &Value, after: i64) -> Option<i64> {
        if v.get("state").and_then(Value::as_str) != Some("ENABLED") {
            return None;
        }
        let schedule = aws_cron::parse(v.get("scheduleExpression")?.as_str()?).ok()?;
        let zone = v
            .get("timezone")
            .and_then(Value::as_str)
            .and_then(aws_cron::Zone::named)
            .unwrap_or_else(aws_cron::Zone::utc);
        let anchor = v.get("creationTime").and_then(Value::as_i64).unwrap_or(0);
        let begin =
            epoch_ms(v.get("scheduleStartTime").and_then(Value::as_i64)).unwrap_or(i64::MIN);
        let next = schedule.next_in(&zone, anchor, after.max(begin.saturating_sub(1)))?;
        let end = epoch_ms(v.get("scheduleEndTime").and_then(Value::as_i64)).unwrap_or(i64::MAX);
        (next <= end).then_some(next)
    }

    /// When the region next has a scheduled query to run.
    pub(crate) fn next_due(&self) -> Option<i64> {
        self.docs
            .with_prefix(&doc_key(SCHEDULED, ""))
            .filter_map(|(_, d)| serde_json::from_str::<Value>(&d.json).ok())
            .filter_map(|v| {
                let last = v
                    .get("_last")
                    .and_then(Value::as_i64)
                    .or_else(|| v.get("creationTime").and_then(Value::as_i64))?;
                Self::due_after(&v, last)
            })
            .min()
    }

    /// Runs the scheduled queries that are due, and says when the next is.
    pub(crate) fn tick(&mut self) -> Option<i64> {
        let now = self.now_ms();
        let due: Vec<(String, Value)> = self
            .docs
            .with_prefix(&doc_key(SCHEDULED, ""))
            .filter_map(|(k, d)| Some((k.to_owned(), serde_json::from_str::<Value>(&d.json).ok()?)))
            .filter(|(_, v)| {
                let last = v
                    .get("_last")
                    .and_then(Value::as_i64)
                    .or_else(|| v.get("creationTime").and_then(Value::as_i64))
                    .unwrap_or(now);
                Self::due_after(v, last).is_some_and(|t| t <= now)
            })
            .collect();
        for (key, v) in due {
            self.run_scheduled(key, v, now);
        }
        self.next_due()
    }

    fn run_scheduled(&mut self, key: String, mut v: Value, now: i64) {
        let text = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let (region, account, name) = (text("_region"), text("_account"), text("name"));
        let listed = identifiers(&v);
        let names: Vec<String> = listed.iter().map(|i| name_of_identifier(i)).collect();
        let offset = |k: &str| {
            v.get(k)
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .saturating_mul(1000)
        };
        let range = (
            now.saturating_sub(offset("startTimeOffset")),
            now.saturating_sub(offset("endTimeOffset")),
        );
        let s3 = v
            .pointer("/destinationConfiguration/s3Configuration/destinationIdentifier")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut record = json!({"triggeredTimestamp": now, "_region": region, "_account": account});
        let started = crate::insights::parse(&text("queryString"))
            .ok()
            .filter(|_| names.iter().all(|n| self.groups.get(n).is_some()));
        match started {
            Some(query) => {
                let deliver = s3
                    .as_deref()
                    .map(|uri| target(uri, &name, &region, &account));
                let plan = Plan {
                    names,
                    listed,
                    text: text("queryString"),
                    query,
                    range,
                    cap: None,
                    region: region.clone(),
                    account: account.clone(),
                };
                match self.begin(plan, deliver) {
                    Ok(id) => record.put("queryId", json!(id)),
                    Err(e) => record.put("errorMessage", json!(e.message)),
                }
                record.put("executionStatus", json!("Running"));
            }
            None => record.put("executionStatus", json!("InvalidQuery")),
        }
        if let Some(uri) = s3 {
            record.put("destinations", json!([{"destinationType": "S3", "destinationIdentifier": uri, "status": "IN_PROGRESS"}]));
        }
        let status = record.get("executionStatus").cloned().unwrap_or_default();
        if let Some(Value::Array(h)) = v.get_mut("_history") {
            h.push(record);
            let over = h.len().saturating_sub(MAX_HISTORY);
            h.drain(..over);
        }
        v.put("_last", json!(now));
        v.put("lastTriggeredTime", json!(now));
        v.put("lastExecutionStatus", status);
        self.keep_scheduled(key, &v);
    }
}

/// A scheduled query's log groups as it names them.
fn identifiers(v: &Value) -> Vec<String> {
    v.get("logGroupIdentifiers")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The bucket and key a run's answer is written to: `s3://bucket/path` gives `path/<name>/<run>.json` (the run id is added later).
fn target(uri: &str, name: &str, region: &str, account: &str) -> Deliver {
    let rest = uri.trim_start_matches("s3://");
    let (bucket, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.trim_matches('/');
    let key = if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}/{name}")
    };
    Deliver {
        bucket: bucket.to_owned(),
        key,
        region: region.to_owned(),
        account: account.to_owned(),
    }
}

/// Writes a finished run's answer to S3 and records how that went.
pub(crate) async fn deliver(peers: Option<&aws_peers::Peers>, run: &Arc<QueryRun>, d: Deliver) {
    let key = format!("{}/{}.json", d.key, run.id);
    let Some(peers) = peers else {
        run.set_delivery(Delivery {
            status: "FAILED",
            key,
            error: Some("S3 is not running in doze-aws.".into()),
        });
        return;
    };
    let rows: Vec<Value> = run.with_state(|s| {
        s.rows
            .iter()
            .map(|r| {
                Value::Object(
                    r.iter()
                        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                        .collect(),
                )
            })
            .collect()
    });
    let caller = Caller {
        access_key_id: String::new(),
        account_id: d.account,
        region: d.region,
        endpoint: String::new(),
        service: Some("logs".into()),
        session_token: None,
    };
    let body = Value::Array(rows).to_string();
    let put = peers
        .call_rest_with_body(
            "logs",
            &caller,
            ("s3", "PutObject"),
            &json!({"Bucket": d.bucket, "Key": key}),
            bytes::Bytes::from(body),
        )
        .await;
    run.set_delivery(match put {
        Ok(_) => Delivery {
            status: "COMPLETE",
            key,
            error: None,
        },
        Err(e) => Delivery {
            status: "FAILED",
            key,
            error: Some(e.message),
        },
    });
}
