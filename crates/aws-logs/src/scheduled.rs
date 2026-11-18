//! Scheduled queries: a Logs Insights query run on a `cron(...)` or `rate(...)` schedule (in its time zone), over the window its
//! offsets give, with its answer written to the local S3. The region's timer is the deadline mechanism every timed service uses
//! (`doze_runtime::Deadlines`): asleep until the next run is due, and gone while no scheduled query is enabled. Each run is an
//! ordinary query (it shows in `DescribeQueries` and `GetQueryResults`), recorded in the query's history (the newest
//! [`MAX_HISTORY`]). The object is `<uri path>/<scheduled query name>/<query id>.json`, the answer as JSON rows (memory: AWS's layout
//! is not in the model). A lookup-table destination is refused: lookup tables are not built.

use std::sync::Arc;

use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Doc, doc_key};
use crate::queries::{Delivery, QueryRun, Status};
use crate::query_ops::{Deliver, Plan};
use crate::req::{Put, Req, name_of_identifier};

pub(crate) const SCHEDULED: &str = "scheduled-query";
/// Runs kept in a scheduled query's history.
const MAX_HISTORY: usize = 100;

fn validation(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}

/// (memory)
fn no_scheduled() -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        "The specified scheduled query does not exist.",
    )
}

/// A time the model calls "Unix epoch format": seconds, or milliseconds when it is that large. (memory)
fn epoch_ms(v: Option<i64>) -> Option<i64> {
    v.map(|t| if t < 100_000_000_000 { t * 1000 } else { t })
}

impl Core {
    fn scheduled(&self, identifier: &str) -> Option<(String, Value)> {
        let name = identifier
            .rsplit_once(":scheduled-query:")
            .map_or(identifier, |(_, id)| id);
        self.docs
            .with_prefix(&doc_key(SCHEDULED, ""))
            .filter_map(|(k, d)| Some((k.to_owned(), serde_json::from_str::<Value>(&d.json).ok()?)))
            .find(|(_, v)| {
                v.get("name").and_then(Value::as_str) == Some(name)
                    || v.get("_id").and_then(Value::as_str) == Some(name)
            })
    }

    fn keep_scheduled(&mut self, key: String, v: &Value) {
        let updated_ms = self.now_ms();
        self.docs.insert(
            key,
            Doc {
                json: v.to_string(),
                updated_ms,
            },
        );
    }

    /// The members `CreateScheduledQuery` and `UpdateScheduledQuery` share, checked.
    fn scheduled_config(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        if r.s("queryLanguage") != "CWLI" {
            return Err(validation(
                "doze-aws runs Logs Insights QL (CWLI) queries only; OpenSearch SQL and PPL are not built.",
            ));
        }
        crate::insights::parse(r.s("queryString"))
            .map_err(|e| validation(format!("The query is malformed: {}", e.message)))?;
        aws_cron::parse(r.s("scheduleExpression"))
            .map_err(|_| validation("The schedule expression is not valid."))?;
        if let Some(tz) = r.opt("timezone").filter(|z| !aws_cron::Zone::known(z)) {
            return Err(validation(format!("The timezone '{tz}' is not valid.")));
        }
        for id in r.strings("logGroupIdentifiers") {
            if self.groups.get(&name_of_identifier(&id)).is_none() {
                return Err(crate::req::no_group());
            }
        }
        if r.v
            .pointer("/destinationConfiguration/lookupTableConfiguration")
            .is_some()
        {
            return Err(validation(
                "Lookup table destinations are not built in doze-aws.",
            ));
        }
        let mut c = json!({
            "queryLanguage": "CWLI",
            "queryString": r.s("queryString"),
            "scheduleExpression": r.s("scheduleExpression"),
            "executionRoleArn": r.s("executionRoleArn"),
            "state": r.opt("state").unwrap_or("ENABLED"),
        });
        for k in ["description", "timezone"] {
            if let Some(v) = r.opt(k) {
                c.put(k, json!(v));
            }
        }
        for k in [
            "logGroupIdentifiers",
            "destinationConfiguration",
            "startTimeOffset",
            "endTimeOffset",
            "scheduleStartTime",
            "scheduleEndTime",
        ] {
            if let Some(v) = r.v.get(k) {
                c.put(k, v.clone());
            }
        }
        Ok(c)
    }

    pub(crate) fn create_scheduled(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("name");
        if self.scheduled(name).is_some() {
            // (memory)
            return Err(ApiError::modeled(
                "ConflictException",
                format!("A scheduled query named '{name}' already exists."),
            ));
        }
        let mut v = self.scheduled_config(r)?;
        let id = aws_seal::uuid_v4().unwrap_or_default();
        let now = self.now_ms();
        let arn = format!(
            "arn:aws:logs:{}:{}:scheduled-query:{id}",
            r.region(),
            r.account()
        );
        for (k, x) in [
            ("scheduledQueryArn", json!(arn)),
            ("name", json!(name)),
            ("scheduleType", json!("CUSTOMER_MANAGED")),
            ("creationTime", json!(now)),
            ("lastUpdatedTime", json!(now)),
            ("_id", json!(id)),
            ("_region", json!(r.region())),
            ("_account", json!(r.account())),
            ("_history", json!([])),
        ] {
            v.put(k, x);
        }
        if let Some(t) = r.v.get("tags") {
            v.put("tags", t.clone());
        }
        let state = v.get("state").cloned().unwrap_or_default();
        self.keep_scheduled(doc_key(SCHEDULED, &id), &v);
        Ok(json!({"scheduledQueryArn": arn, "state": state}))
    }

    pub(crate) fn get_scheduled(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (_, v) = self.scheduled(r.s("identifier")).ok_or_else(no_scheduled)?;
        Ok(self.shown_scheduled(&v))
    }

    pub(crate) fn update_scheduled(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (key, old) = self.scheduled(r.s("identifier")).ok_or_else(no_scheduled)?;
        let mut v = self.scheduled_config(r)?;
        for k in [
            "scheduledQueryArn",
            "name",
            "scheduleType",
            "creationTime",
            "lastTriggeredTime",
            "lastExecutionStatus",
            "tags",
            "_id",
            "_region",
            "_account",
            "_history",
            "_last",
        ] {
            if let Some(x) = old.get(k) {
                v.put(k, x.clone());
            }
        }
        v.put("lastUpdatedTime", json!(self.now_ms()));
        self.keep_scheduled(key, &v);
        Ok(self.shown_scheduled(&v))
    }

    pub(crate) fn delete_scheduled(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (key, _) = self.scheduled(r.s("identifier")).ok_or_else(no_scheduled)?;
        self.docs.remove(&key);
        Ok(json!({}))
    }

    pub(crate) fn list_scheduled(&self, r: &Req<'_>) -> Value {
        let (state, kind) = (r.opt("state"), r.opt("scheduleType"));
        let all: Vec<Value> = self
            .docs
            .with_prefix(&doc_key(SCHEDULED, ""))
            .filter_map(|(_, d)| serde_json::from_str::<Value>(&d.json).ok())
            .filter(|v| state.is_none_or(|s| v.get("state").and_then(Value::as_str) == Some(s)))
            .filter(|v| {
                kind.is_none_or(|s| v.get("scheduleType").and_then(Value::as_str) == Some(s))
            })
            .map(|v| {
                let full = self.shown_scheduled(&v);
                let keep = [
                    "scheduledQueryArn",
                    "name",
                    "state",
                    "scheduleType",
                    "lastTriggeredTime",
                    "lastExecutionStatus",
                    "scheduleExpression",
                    "timezone",
                    "destinationConfiguration",
                    "creationTime",
                    "lastUpdatedTime",
                ];
                Value::Object(
                    full.as_object()
                        .map(|m| {
                            m.iter()
                                .filter(|(k, _)| keep.contains(&k.as_str()))
                                .map(|(k, x)| (k.clone(), x.clone()))
                                .collect()
                        })
                        .unwrap_or_default(),
                )
            })
            .collect();
        let size = r
            .int("maxResults")
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(1000);
        let from = r
            .opt("nextToken")
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(0);
        let mut out =
            json!({"scheduledQueries": all.iter().skip(from).take(size).collect::<Vec<_>>()});
        if from + size < all.len() {
            out.put("nextToken", json!((from + size).to_string()));
        }
        out
    }

    pub(crate) fn scheduled_history(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (_, v) = self.scheduled(r.s("identifier")).ok_or_else(no_scheduled)?;
        let (from, to) = (
            r.int("startTime").unwrap_or(0),
            r.int("endTime").unwrap_or(i64::MAX),
        );
        let (from, to) = (
            epoch_ms(Some(from)).unwrap_or(0),
            epoch_ms(Some(to)).unwrap_or(i64::MAX),
        );
        let statuses = r.strings("executionStatuses");
        let records: Vec<Value> = v
            .get("_history")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|h| {
                h.get("triggeredTimestamp")
                    .and_then(Value::as_i64)
                    .is_some_and(|t| t >= from && t <= to)
            })
            .map(|h| self.resolved(h))
            .filter(|h| {
                statuses.is_empty()
                    || statuses
                        .iter()
                        .any(|s| h.get("executionStatus").and_then(Value::as_str) == Some(s))
            })
            .collect();
        let size = r
            .int("maxResults")
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(1000);
        let start = r
            .opt("nextToken")
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(0);
        let mut out = json!({
            "name": v.get("name").cloned().unwrap_or_default(),
            "scheduledQueryArn": v.get("scheduledQueryArn").cloned().unwrap_or_default(),
            "triggerHistory": records.iter().skip(start).take(size).collect::<Vec<_>>(),
        });
        if start + size < records.len() {
            out.put("nextToken", json!((start + size).to_string()));
        }
        Ok(out)
    }

    /// A history record with the run's status as it is now.
    fn resolved(&self, h: &Value) -> Value {
        let mut h = h.clone();
        let id = h
            .get("queryId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let (region, account) = (
            h.get("_region")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            h.get("_account")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        );
        if let Some(run) = self.queries.get(&id, &region, &account) {
            let status = match run.status() {
                Status::Scheduled | Status::Running => "Running",
                Status::Complete => "Complete",
                Status::Failed | Status::Cancelled => "Failed",
            };
            h.put("executionStatus", json!(status));
            if let Some(why) = run.with_state(|s| s.failure.clone()) {
                h.put("errorMessage", json!(why));
            }
            if let (Some(d), Some(Value::Array(dests))) =
                (run.delivery(), h.get_mut("destinations"))
            {
                for dest in dests {
                    dest.put("status", json!(d.status));
                    dest.put("processedIdentifier", json!(d.key));
                    if let Some(e) = &d.error {
                        dest.put("errorMessage", json!(e));
                    }
                }
            }
        }
        if let Value::Object(m) = &mut h {
            m.retain(|k, _| !k.starts_with('_'));
        }
        h
    }

    /// A scheduled query as `GetScheduledQuery` shows it: its last run's status brought up to date, and nothing kept for doze.
    fn shown_scheduled(&self, v: &Value) -> Value {
        let mut out = v.clone();
        if let Some(last) = v
            .get("_history")
            .and_then(Value::as_array)
            .and_then(|h| h.last())
        {
            let now = self.resolved(last);
            if let Some(s) = now.get("executionStatus") {
                out.put("lastExecutionStatus", s.clone());
            }
        }
        if let Value::Object(m) = &mut out {
            m.retain(|k, _| !k.starts_with('_') && k != "tags");
        }
        out
    }

    /// The tags of a scheduled query after `change`, kept only within the tag limit (`None` when there is no such query).
    pub(crate) fn scheduled_tags(
        &mut self,
        arn: &str,
        change: impl FnOnce(&mut serde_json::Map<String, Value>),
    ) -> Option<Value> {
        if !arn.contains(":scheduled-query:") {
            return None;
        }
        let (key, mut v) = self.scheduled(arn)?;
        let tags = crate::destinations::changed_tags(&mut v, change);
        if tags
            .as_object()
            .is_some_and(|m| m.len() <= crate::groups::MAX_TAGS)
        {
            self.keep_scheduled(key, &v);
        }
        Some(tags)
    }

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
        let names: Vec<String> = v
            .get("logGroupIdentifiers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(name_of_identifier)
                    .collect()
            })
            .unwrap_or_default();
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
