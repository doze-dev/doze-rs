//! Scheduled queries: a Logs Insights query run on a `cron(...)` or `rate(...)` schedule (in its time zone), over the window its
//! offsets give, with its answer written to the local S3. The region's timer is the deadline mechanism every timed service uses
//! (`doze_runtime::Deadlines`): asleep until the next run is due, and gone while no scheduled query is enabled. Each run is an
//! ordinary query (it shows in `DescribeQueries` and `GetQueryResults`), recorded in the query's history (the newest
//! [`MAX_HISTORY`]). The object is `<uri path>/<scheduled query name>/<query id>.json`, the answer as JSON rows (memory: AWS's layout
//! is not in the model). A lookup-table destination is refused: lookup tables are not built.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Doc, doc_key};
use crate::queries::Status;
use crate::req::{Put, Req, backend, name_of_identifier};

pub(crate) const SCHEDULED: &str = "scheduled-query";
/// Runs kept in a scheduled query's history.
pub(crate) const MAX_HISTORY: usize = 100;

fn validation(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}

/// Recorded 2026-11-19: the identifier as given.
fn no_scheduled(identifier: &str) -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        format!("Scheduled query with identifier {identifier} not found"),
    )
}

/// A time the model calls "Unix epoch format": seconds, or milliseconds when it is that large. (memory)
pub(crate) fn epoch_ms(v: Option<i64>) -> Option<i64> {
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

    pub(crate) fn keep_scheduled(&mut self, key: String, v: &Value) {
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
        // Recorded 2026-11-19 (UpdateScheduledQuery); the model marks it optional.
        if r.v.get("startTimeOffset").is_none() {
            return Err(validation(
                "1 validation errors detected: StartTimeOffset is required field",
            ));
        }
        if r.s("queryLanguage") != "CWLI" {
            return Err(validation(
                "doze-aws runs Logs Insights QL (CWLI) queries only; OpenSearch SQL and PPL are not built.",
            ));
        }
        let q = r.s("queryString");
        if r.strings("logGroupIdentifiers").is_empty() {
            // Recorded 2026-11-19: the query back end's refusal, passed on as a ValidationException.
            let e = crate::insights::without_groups(q);
            return Err(validation(backend(
                "MalformedQueryException",
                &e.message(q, 0),
            )));
        }
        // How AWS words a malformed scheduled query that names its groups is (memory): as the query back end does.
        crate::insights::parse(q)
            .map_err(|e| validation(backend("MalformedQueryException", &e.message(q, 0))))?;
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
        if c.get("endTimeOffset").is_none() {
            // Recorded 2026-11-19: shown as 0 when not given.
            c.put("endTimeOffset", json!(0));
        }
        Ok(c)
    }

    pub(crate) fn create_scheduled(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("name");
        // Recorded 2026-11-19: the request is checked before the name is.
        let mut v = self.scheduled_config(r)?;
        if self.scheduled(name).is_some() {
            // (memory)
            return Err(ApiError::modeled(
                "ConflictException",
                format!("A scheduled query named '{name}' already exists."),
            ));
        }
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
        let (_, v) = self
            .scheduled(r.s("identifier"))
            .ok_or_else(|| no_scheduled(r.s("identifier")))?;
        Ok(self.shown_scheduled(&v))
    }

    pub(crate) fn update_scheduled(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (key, old) = self
            .scheduled(r.s("identifier"))
            .ok_or_else(|| no_scheduled(r.s("identifier")))?;
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
        let (key, _) = self
            .scheduled(r.s("identifier"))
            .ok_or_else(|| no_scheduled(r.s("identifier")))?;
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
        let (_, v) = self
            .scheduled(r.s("identifier"))
            .ok_or_else(|| no_scheduled(r.s("identifier")))?;
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
}
