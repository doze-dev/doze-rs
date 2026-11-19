//! The Logs Insights operations: `StartQuery`, `GetQueryResults`, `StopQuery`, `DescribeQueries`, `ListLogGroupsForQuery`,
//! `GetLogRecord` and `GetLogGroupFields`. A query is parsed and its streams chosen in the actor; it is read from a snapshot of the
//! event table by `insights::job`, off the actor, and its answer kept in the process's [`crate::queries::Registry`].

use std::sync::Arc;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::insights::job::{Job, Source};
use crate::insights::{self, record};
use crate::model::{Event, group_arn, stream_key};
use crate::queries::{MAX_CONCURRENT, QueryRun, Status};
use crate::req::{Put, Req, invalid, name_of_identifier};

/// The most log groups one query reads (the model: "You can include up to 50 log groups").
const MAX_GROUPS: usize = 50;
/// Rows a query answers when neither `limit` nor the query says (memory: Insights' default of 1,000).
const DEFAULT_ROWS: usize = 1000;
/// Rows a query whose own `limit` sets the size may answer (memory: Insights shows at most 10,000 rows).
const MOST_ROWS: usize = 10_000;
/// Rows one `GetQueryResults` page holds (the model's `maxItems` maximum).
const PAGE: usize = 10_000;

/// A query to run: its groups, text, parsed form, time range (milliseconds, both ends included) and size.
#[derive(Debug)]
pub(crate) struct Plan {
    pub(crate) names: Vec<String>,
    pub(crate) text: String,
    pub(crate) query: insights::ast::Query,
    pub(crate) range: (i64, i64),
    pub(crate) cap: Option<usize>,
    pub(crate) region: String,
    pub(crate) account: String,
}

/// Where a scheduled query's answer goes: a bucket and key of the local S3, as the account and region.
#[derive(Debug)]
pub(crate) struct Deliver {
    pub(crate) bucket: String,
    pub(crate) key: String,
    pub(crate) region: String,
    pub(crate) account: String,
}

/// (memory)
fn no_query() -> ApiError {
    ApiError::modeled("ResourceNotFoundException", "Query does not exist.")
}

impl Core {
    /// The groups a `StartQuery` names: exactly one of `logGroupName`, `logGroupNames` and `logGroupIdentifiers`. Wording (memory).
    fn query_groups(&self, r: &Req<'_>) -> Result<Vec<String>, ApiError> {
        let given = [
            r.opt("logGroupName").map(|n| vec![n.to_owned()]),
            r.v.get("logGroupNames").map(|_| r.strings("logGroupNames")),
            r.v.get("logGroupIdentifiers").map(|_| {
                r.strings("logGroupIdentifiers")
                    .iter()
                    .map(|i| name_of_identifier(i))
                    .collect()
            }),
        ];
        let mut named = given.into_iter().flatten();
        let (Some(names), None) = (named.next(), named.next()) else {
            return Err(invalid(
                "Exactly one of logGroupName, logGroupNames and logGroupIdentifiers must be specified.",
            ));
        };
        if names.is_empty() || names.len() > MAX_GROUPS {
            return Err(invalid(format!(
                "A query can include from 1 to {MAX_GROUPS} log groups."
            )));
        }
        for n in &names {
            if self.groups.get(n).is_none() {
                return Err(ApiError::modeled(
                    "ResourceNotFoundException",
                    format!(
                        "Log group '{n}' does not exist for account ID '{}'",
                        r.account()
                    ),
                ));
            }
        }
        Ok(names)
    }

    pub(crate) fn start_query(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        if r.opt("queryLanguage").is_some_and(|l| l != "CWLI") {
            return Err(invalid(
                "doze-aws runs Logs Insights QL (CWLI) queries only; OpenSearch SQL and PPL are not built.",
            ));
        }
        let names = self.query_groups(r)?;
        let (start, end) = (
            r.int("startTime").unwrap_or(0),
            r.int("endTime").unwrap_or(0),
        );
        if end < start {
            // (memory)
            return Err(invalid("End time should not be before the start time."));
        }
        let query = insights::parse(r.s("queryString"))
            .map_err(|e| ApiError::modeled("MalformedQueryException", e.message.clone()))?;
        if self.queries.live(r.account()) >= MAX_CONCURRENT {
            // (memory)
            return Err(ApiError::modeled(
                "LimitExceededException",
                format!("Account maximum query concurrency limit of [{MAX_CONCURRENT}] reached."),
            ));
        }
        let plan = Plan {
            names,
            text: r.s("queryString").to_owned(),
            cap: r.int("limit").and_then(|n| usize::try_from(n).ok()),
            query,
            range: (
                start.saturating_mul(1000),
                end.saturating_mul(1000).saturating_add(999),
            ),
            region: r.region().to_owned(),
            account: r.account().to_owned(),
        };
        let id = self.begin(plan, None)?;
        Ok(json!({"queryId": id}))
    }

    /// Starts a query run, and its delivery once it ends (a scheduled query's): the run's id.
    pub(crate) fn begin(
        &mut self,
        plan: Plan,
        deliver: Option<Deliver>,
    ) -> Result<String, ApiError> {
        let own = plan
            .query
            .cmds
            .iter()
            .any(|c| matches!(c, insights::ast::Cmd::Limit(_)));
        let cap = plan
            .cap
            .unwrap_or(if own { MOST_ROWS } else { DEFAULT_ROWS });
        let id = aws_seal::uuid_v4().unwrap_or_default();
        let region = (plan.region.as_str(), plan.account.as_str());
        let run = Arc::new(QueryRun::new(
            id.clone(),
            region,
            &plan.text,
            plan.names.clone(),
            self.now_ms(),
        ));
        let job = Job {
            run: Arc::clone(&run),
            snapshot: self
                .events
                .snapshot(self.db.as_ref())
                .map_err(|e| ApiError::internal(e.to_string()))?,
            sources: self.sources(&plan.names, &plan.region),
            from_ms: self.query_floor(&plan.names, plan.range.0),
            to_ms: plan.range.1,
            query: plan.query,
            account: plan.account,
            cap,
        };
        self.queries.add(run);
        self.launch(job, deliver);
        Ok(id)
    }

    /// The streams of the named groups.
    fn sources(&self, names: &[String], region: &str) -> Vec<Source> {
        let mut out = Vec::new();
        for g in names {
            out.extend(
                self.streams
                    .with_prefix(&stream_key(g, ""))
                    .map(|(_, s)| Source {
                        no: s.no,
                        name: s.name.clone(),
                        group: g.clone(),
                        region: region.to_owned(),
                    }),
            );
        }
        out
    }

    /// The start of a query's range, moved past what the groups' retention and the local age bound have removed (or will on their
    /// next sweep).
    fn query_floor(&self, names: &[String], start_ms: i64) -> i64 {
        let now = self.now_ms();
        let local = self.keep_ms.map_or(i64::MIN, |k| now.saturating_sub(k));
        let shortest = names
            .iter()
            .filter_map(|n| self.groups.get(n).and_then(|g| g.retention_days))
            .min()
            .map_or(i64::MIN, |d| now - d * 86_400_000);
        start_ms.max(local).max(shortest)
    }

    /// Starts a run: on the blocking pool, after its turn, when the service has a runtime; at once otherwise (under test). Then
    /// its delivery, if it has one.
    fn launch(&self, job: Job, deliver: Option<Deliver>) {
        let run = Arc::clone(&job.run);
        let Some(tasks) = &self.tasks else {
            job.execute();
            if let Some(d) = deliver {
                run.set_delivery(crate::queries::Delivery {
                    status: "FAILED",
                    key: d.key,
                    error: Some("S3 is not running in doze-aws.".into()),
                });
            }
            return;
        };
        let (turns, peers) = (Arc::clone(&self.queries.turns), self.peers.clone());
        tasks.spawn(async move {
            let Ok(turn) = turns.acquire_owned().await else {
                return;
            };
            if tokio::task::spawn_blocking(move || job.execute())
                .await
                .is_err()
            {
                // WHY: a run that panicked leaves its query Running; nothing else waits on it.
            }
            drop(turn);
            if let Some(d) = deliver {
                crate::scheduled_run::deliver(peers.as_deref(), &run, d).await;
            }
        });
    }

    pub(crate) fn get_query_results(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let run = self
            .queries
            .get(r.s("queryId"), r.region(), r.account())
            .ok_or_else(no_query)?;
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
            .ok_or_else(no_query)?;
        let stopped = run.with_state(|s| {
            let live = s.status.live();
            if live {
                s.status = Status::Cancelled;
            }
            live
        });
        if !stopped {
            // The model: "If the query has already ended, the operation returns an error indicating that the specified query is not
            // running." Code and wording (memory).
            return Err(invalid(format!("Query {} is not running.", run.id)));
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
            .ok_or_else(no_query)?;
        let size = r
            .int("maxResults")
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(50);
        let from = r
            .opt("nextToken")
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(0);
        let arns: Vec<String> = run
            .groups
            .iter()
            .skip(from)
            .take(size)
            .map(|g| group_arn(r.region(), r.account(), g))
            .collect();
        let mut out = json!({"logGroupIdentifiers": arns});
        if from + size < run.groups.len() {
            out.put("nextToken", json!((from + size).to_string()));
        }
        Ok(out)
    }

    /// Every field of the event a `@ptr` names, `@timestamp` and `@ingestionTime` as milliseconds. (memory)
    pub(crate) fn get_log_record(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        // (memory)
        let bad = || invalid("The specified logRecordPointer is not valid.");
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
        let stream = self
            .streams
            .iter()
            .find(|(_, s)| s.no == no && s.group == group)
            .map(|(_, s)| s.name.clone())
            .ok_or_else(gone)?;
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
        Ok(json!({"logRecord": fields}))
    }
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
