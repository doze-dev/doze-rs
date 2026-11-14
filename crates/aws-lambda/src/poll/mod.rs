//! Event source mappings at work. A poller is a task for one enabled mapping; it waits for its source to say it has something (SQS
//! says a message arrived, Kinesis that records were appended, DynamoDB that a stream grew), reads a batch, filters it, invokes the
//! function and acts on the answer: delete what succeeded (SQS), move the position past what succeeded (streams), retry, bisect or
//! send a failed batch to the failure destination. With no mapping enabled there is no task and no timer; the regions that have
//! one are remembered in `mappings.armed`, so a restart resumes them.

pub(crate) mod filter;
mod kinesis;
mod parallel;
pub(crate) mod sqs;
mod streams;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use aws_wire::service::Caller;
use bytes::Bytes;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::Inner;
use crate::core::{Answer, Ask};
use crate::invoke::Input;
use crate::mappings::Source;
use crate::resolve::Resolving;

const ARMED: &str = "mappings.armed";

/// The pollers running, by `region/uuid`, with the generation of the mapping each was started for.
#[derive(Debug, Default)]
pub(crate) struct Pollers {
    running: Mutex<HashMap<String, (String, CancellationToken)>>,
}

/// Everything one poller works with.
#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub(crate) inner: Arc<Inner>,
    pub(crate) region: String,
    pub(crate) uuid: String,
    pub(crate) cfg: Value,
    pub(crate) caller: Caller,
    pub(crate) stop: CancellationToken,
    /// Iterators made when the mapping was created, so a record put right after is not missed by a poller that has not started.
    pub(crate) seed: HashMap<String, String>,
}

/// What invoking the function for a batch came to.
#[derive(Debug)]
pub(crate) struct Invoked {
    /// The function failed (an error, a timeout, or it could not be run).
    pub(crate) failed: bool,
    pub(crate) payload: Bytes,
    pub(crate) executed: String,
    pub(crate) request_id: String,
    /// The function was run (and not merely looked for).
    pub(crate) ran: bool,
}

impl Run {
    pub(crate) fn int(&self, key: &str, default: i64) -> i64 {
        self.cfg.get(key).and_then(crate::int).unwrap_or(default)
    }

    pub(crate) fn text(&self, key: &str) -> &str {
        self.cfg
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    pub(crate) fn source_arn(&self) -> &str {
        self.text("EventSourceArn")
    }

    pub(crate) fn reports_failures(&self) -> bool {
        self.cfg
            .get("FunctionResponseTypes")
            .and_then(Value::as_array)
            .is_some_and(|a| {
                a.iter()
                    .any(|v| v.as_str() == Some("ReportBatchItemFailures"))
            })
    }

    /// Whether a record passes the mapping's filters (all pass when there are none).
    pub(crate) fn passes(&self, doc: &Value) -> bool {
        let filters: Vec<Value> = self
            .cfg
            .pointer("/FilterCriteria/Filters")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|f| serde_json::from_str(f.get("Pattern")?.as_str()?).ok())
                    .collect()
            })
            .unwrap_or_default();
        filters.is_empty() || filters.iter().any(|p| filter::matches(p, doc))
    }

    /// Calls the function with a batch of records.
    pub(crate) async fn invoke(&self, records: &[Value]) -> Invoked {
        let failed = |why: String| Invoked {
            failed: true,
            payload: Bytes::from(json!({"errorMessage": why}).to_string()),
            executed: String::new(),
            request_id: String::new(),
            ran: false,
        };
        let ask = Resolving {
            caller: self.caller.clone(),
            function: self.text("FunctionArn").to_owned(),
            qualifier: None,
            source_arn: None,
            draw: 0.0,
            url: false,
            internal: true,
        };
        let rs = match self
            .inner
            .ask(&self.region, Ask::Resolve(Box::new(ask)))
            .await
        {
            Ok(Answer::Resolved(Ok(rs))) => rs,
            Ok(Answer::Resolved(Err(e))) => return failed(e.message),
            _ => return failed("the function could not be found".to_owned()),
        };
        let payload = Bytes::from(json!({"Records": records}).to_string());
        let input = Input {
            payload,
            client_context: None,
            trace_id: String::new(),
        };
        let out = match self.inner.run_function(&rs, &self.caller, input).await {
            Ok(out) => out,
            Err(e) => return failed(e.message),
        };
        Invoked {
            failed: out.error,
            payload: out.payload,
            executed: rs.version.clone(),
            request_id: out.request_id,
            ran: true,
        }
    }

    /// Records what the last invocation did (`LastProcessingResult`).
    pub(crate) async fn note(&self, result: &str) {
        let result = match result {
            "FAILED" => "PROBLEM: Function call failed",
            "PARTIAL" => "PROBLEM: Some items of the batch failed",
            other => other,
        };
        drop(
            self.inner
                .ask(
                    &self.region,
                    Ask::Result {
                        uuid: self.uuid.clone(),
                        result: result.to_owned(),
                    },
                )
                .await,
        );
    }

    pub(crate) async fn position(&self, key: &str) -> Option<String> {
        match self
            .inner
            .ask(&self.region, Ask::Position(format!("{}/{key}", self.uuid)))
            .await
        {
            Ok(Answer::Value(Some(Value::String(s)))) => Some(s),
            _ => None,
        }
    }

    pub(crate) async fn save_position(&self, key: &str, value: &str) {
        let ask = Ask::Checkpoint {
            key: format!("{}/{key}", self.uuid),
            value: Some(value.to_owned()),
        };
        if let Ok(Answer::Reply {
            durable: Some(saved),
            ..
        }) = self.inner.ask(&self.region, ask).await
        {
            drop(saved.await);
        }
    }

    /// Waits `d`, or until the poller is stopped (`false`).
    pub(crate) async fn pause(&self, d: Duration) -> bool {
        tokio::select! {
            () = tokio::time::sleep(d) => true,
            () = self.stop.cancelled() => false,
        }
    }

    /// Sends a failed batch's description to the mapping's failure destination.
    pub(crate) async fn on_failure(&self, doc: &Value) {
        let Some(target) = self
            .cfg
            .pointer("/DestinationConfig/OnFailure/Destination")
            .and_then(Value::as_str)
        else {
            return;
        };
        let ask = Resolving {
            caller: self.caller.clone(),
            function: self.text("FunctionArn").to_owned(),
            qualifier: None,
            source_arn: None,
            draw: 0.0,
            url: false,
            internal: true,
        };
        let Ok(Answer::Resolved(Ok(rs))) = self
            .inner
            .ask(&self.region, Ask::Resolve(Box::new(ask)))
            .await
        else {
            return;
        };
        if let Err(why) = self
            .inner
            .send_to(&self.caller, target, &doc.to_string(), &rs)
            .await
        {
            tracing::warn!(mapping = %self.uuid, "could not send the failed batch to {target}: {why}");
        }
    }
}

/// A task that waits for a source to say it has something, ended when dropped.
pub(crate) struct Wake(tokio::task::JoinHandle<()>);

impl Wake {
    /// Starts waiting: the task is spawned, and the caller yields once before reading, so the wait is in place before the read it guards.
    pub(crate) fn on<F: std::future::Future<Output = ()> + Send + 'static>(wait: F) -> Self {
        Self(tokio::spawn(wait))
    }

    /// Resolves when the source has said so.
    pub(crate) async fn fired(&mut self) {
        drop((&mut self.0).await);
    }
}

impl Drop for Wake {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Starts and stops pollers so they are the enabled mappings of `region`.
pub(crate) async fn sync(inner: &Arc<Inner>, region: &str) {
    let Ok(Answer::Many(list)) = inner.ask(region, Ask::Enabled).await else {
        return;
    };
    let marker = inner.region_dir(region).join(ARMED);
    if list.is_empty() {
        drop(std::fs::remove_file(&marker));
    } else {
        drop(std::fs::create_dir_all(inner.region_dir(region)));
        drop(std::fs::write(&marker, "1"));
    }
    let wanted: HashMap<String, Value> = list
        .into_iter()
        .filter_map(|m| Some((format!("{region}/{}", m.get("UUID")?.as_str()?), m)))
        .collect();
    let starts = reconcile(inner, region, wanted);
    for run in starts {
        let run = streams::prepare(run).await;
        inner.tasks.spawn(poller(run));
    }
}

/// Stops the pollers that are no longer wanted (or are for an older version of their mapping), and returns the ones to start.
fn reconcile(inner: &Arc<Inner>, region: &str, wanted: HashMap<String, Value>) -> Vec<Run> {
    let mut starts: Vec<Run> = Vec::new();
    let mut running = inner
        .pollers
        .running
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    running.retain(|key, (generation, token)| {
        let keep = key.starts_with(&format!("{region}/")).then(|| {
            wanted
                .get(key)
                .is_some_and(|m| &generation_of(m) == generation)
        });
        let keep = keep.unwrap_or(true);
        if !keep {
            token.cancel();
        }
        keep
    });
    for (key, mapping) in wanted {
        if running.contains_key(&key) || inner.sources.get().is_none() {
            continue;
        }
        let token = inner.tasks.stopping().child_token();
        running.insert(key.clone(), (generation_of(&mapping), token.clone()));
        let uuid = key.rsplit('/').next().unwrap_or_default().to_owned();
        let account = mapping
            .get("AccountId")
            .and_then(Value::as_str)
            .unwrap_or("000000000000")
            .to_owned();
        let run = Run {
            inner: Arc::clone(inner),
            region: region.to_owned(),
            uuid,
            caller: Caller {
                access_key_id: "test".to_owned(),
                account_id: account,
                region: region.to_owned(),
                endpoint: String::new(),
                service: Some("lambda".to_owned()),
                session_token: None,
            },
            cfg: mapping,
            stop: token,
            seed: HashMap::new(),
        };
        starts.push(run);
    }
    starts
}

fn generation_of(mapping: &Value) -> String {
    mapping
        .get("LastModified")
        .map(Value::to_string)
        .unwrap_or_default()
}

/// Resumes the regions whose files say a mapping was enabled.
pub(crate) async fn resume(inner: &Arc<Inner>) {
    let mut regions = Vec::new();
    if inner.dir.join(ARMED).exists() {
        regions.push(crate::HOME_REGION.to_owned());
    }
    if let Ok(read) = std::fs::read_dir(inner.dir.join("regions")) {
        for entry in read.filter_map(Result::ok) {
            if entry.path().join(ARMED).exists() {
                regions.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    for region in regions {
        sync(inner, &region).await;
    }
}

async fn poller(run: Run) {
    let Some(sources) = run.inner.sources.get().cloned() else {
        return;
    };
    match Source::of(run.source_arn()) {
        Some(Source::Sqs) => sqs::run(run, sources.sqs).await,
        Some(Source::Kinesis) => kinesis::kinesis(run, sources.kinesis).await,
        Some(Source::DynamoDb) => streams::dynamodb(run, sources.dynamodb).await,
        None => {}
    }
}
