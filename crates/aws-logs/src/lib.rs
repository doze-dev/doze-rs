//! CloudWatch Logs: log groups, streams and events as a state machine behind a lazy actor. State is on disk and survives an idle
//! reap and a restart: groups, streams and the stored documents are held in memory (a few hundred records), and the events are
//! only ever read from the database, so a stream's size is bounded by its retention and not by memory.
//!
//! Built: groups, streams, `PutLogEvents` (the sequence token is ignored, as AWS now does), `GetLogEvents`, `FilterLogEvents` with
//! term, JSON and space-delimited patterns, retention (applied lazily, when a group is written or read), tags, the KMS key,
//! deletion protection, metric filters (a matching event becomes a CloudWatch data point when CloudWatch is running, and a batch
//! with no match reports the filter's `defaultValue`), the Embedded Metric Format (a JSON line with `_aws.CloudWatchMetrics` that
//! Lambda writes becomes CloudWatch data points: `emf`), subscription filters (they deliver to Lambda and Kinesis, directly or
//! through a destination: `forward`, `destinations`), stored documents (resource, account and data protection policies, query
//! definitions), Logs Insights (`insights`, `query_ops`), Live Tail (`tail`), export tasks to S3 (`export`), transformers applied at
//! ingestion (`transform`, `transformers`), field index policies (`indexes`) and scheduled queries (`scheduled`). Refused by name:
//! import tasks, deliveries, anomaly detectors, integrations, lookup tables and the other operations the compatibility notes list as
//! not planned.
//!
//! A group keeps its events forever unless it has a retention, as on AWS; the store itself is bounded by [`Settings`] (a week and
//! 100,000 events per group by default), which is doze's bound and not reported through the API.

mod authz;
mod bounds;
mod core;
mod destinations;
mod dispatch;
mod docs;
mod emf;
mod events;
mod export;
mod fields;
mod filters;
mod forward;
mod groups;
mod indexes;
mod insights;
mod model;
mod pattern;
mod queries;
mod query_ops;
mod query_read;
mod reading;
mod req;
mod scheduled;
mod scheduled_run;
mod storage;
mod streams;
mod tail;
mod transform;
mod transformers;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Duration;

use aws_model_cloudwatch_logs::Operation;
use aws_model_cloudwatch_logs::meta::META;
use aws_model_cloudwatch_logs::tables::TABLES;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::rest::Download;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_runtime::{Ctx, Deadlines, Keyed};

use crate::core::{Answer, Ask, Core};

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// Local bounds on what the store keeps. They are doze's, not AWS's: AWS keeps a group's events forever unless the group has a
/// retention, and that is what the API reports here too (`retentionInDays` is absent until set). These only stop a local store, in
/// which every Lambda invocation writes, from growing without end. Applied lazily, when a group is written or read; nothing ticks.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Events older than this are removed, whatever the group's retention (`--logs-retention-days`; default 7, `0` keeps them).
    pub retention: Option<Duration>,
    /// The most events one group holds; past it the oldest go (`--logs-max-events`; default 100,000, `0` is no cap).
    pub max_events: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            retention: Some(Duration::from_secs(7 * 86_400)),
            max_events: 100_000,
        }
    }
}

/// What every region's actor is handed in its [`Ctx`]: the bus, the settings, and the process's Insights queries.
#[derive(Debug)]
pub(crate) struct Ext {
    pub(crate) peers: Option<Arc<aws_peers::Peers>>,
    pub(crate) settings: Settings,
    pub(crate) queries: Arc<queries::Registry>,
}

/// The CloudWatch Logs service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Logs {
    /// One actor per region: a log group belongs to the region that made it.
    actors: Keyed<Core>,
    ctx: Arc<Ctx>,
    /// One timer per region, asleep until its next scheduled query is due, and none while it has none enabled.
    due: Arc<Deadlines<Core>>,
}

impl Logs {
    /// Registers CloudWatch Logs. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self::with_settings(ctx, Settings::default())
    }

    /// Registers CloudWatch Logs with local bounds.
    #[must_use]
    pub fn with_settings(ctx: Arc<Ctx>, settings: Settings) -> Self {
        let ctx = Arc::new(Ctx {
            dir: ctx.dir.clone(),
            idle: ctx.idle,
            tasks: ctx.tasks.clone(),
            clock: Arc::clone(&ctx.clock),
            ext: Some(Arc::new(Ext {
                peers: core::peers_of(&ctx),
                settings,
                queries: Arc::new(queries::Registry::default()),
            })),
        });
        let due = Deadlines::new(
            Arc::clone(&ctx),
            || Ask::Tick,
            Box::new(|mut answer: Answer| {
                Box::pin(async move {
                    if let Some(saved) = answer.durable.take() {
                        // WHY: a failed save is the writer's to report; the next tick finds the same state.
                        drop(saved.await);
                    }
                    answer.next_due
                })
            }),
        );
        Self {
            actors: Keyed::new("logs", Arc::clone(&ctx), HOME_REGION),
            ctx,
            due,
        }
    }

    /// Starts the scheduled-query timers of the regions that have Logs state on disk. Run once when the process starts: nothing else
    /// would wake a region nobody calls after a restart. A region with nothing scheduled is opened and left to be reaped.
    pub async fn resume(&self) {
        let mut regions = Vec::new();
        if self.ctx.dir.join(core::FILE).exists() {
            regions.push(HOME_REGION.to_owned());
        }
        if let Ok(entries) = std::fs::read_dir(self.ctx.dir.join("regions")) {
            for e in entries.flatten() {
                if e.path().join(core::FILE).exists()
                    && let Some(name) = e.file_name().to_str()
                {
                    regions.push(name.to_owned());
                }
            }
        }
        for region in regions {
            let Ok(actor) = self.actors.actor(&region) else {
                continue;
            };
            if let Ok(mut answer) = actor.call(Ask::Tick).await {
                if let Some(saved) = answer.durable.take() {
                    drop(saved.await);
                }
                self.due.arm(&region, &actor, answer.next_due);
            }
        }
    }

    /// Sends the call to the actor of the region it was signed for, and holds the answer until what it changed is on disk.
    async fn ask(&self, call: Call) -> Reply {
        let region = call.caller.region.clone();
        let actor = match self.actors.actor(&region) {
            Ok(actor) => actor,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        let mut answer = match actor.call(Ask::Call(Box::new(call))).await {
            Ok(answer) => answer,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        self.due.arm(&region, &actor, answer.next_due);
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            return Err(ApiError::internal(format!("could not save: {e}")));
        }
        answer.reply
    }

    /// Opens a Live Tail session in the call's region and streams it.
    async fn tail(&self, call: Call) -> Result<Download, ApiError> {
        let actor = self
            .actors
            .actor(&call.caller.region)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (events, received) = tokio::sync::mpsc::channel(64);
        let (reply, opened) = tokio::sync::oneshot::channel();
        let ask = tail::TailAsk {
            call,
            events,
            reply,
        };
        actor
            .call(Ask::Tail(Box::new(ask)))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let start = opened
            .await
            .map_err(|_| ApiError::internal("the session was not opened"))??;
        Ok(tail::stream(&self.ctx.tasks, start, received))
    }
}

/// Whether `query` parses as a Logs Insights query and, run over `message` as one event, gives a row. Exists so the query parser
/// and the engine can be fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_insights_query(query: &str, message: &str) -> bool {
    insights::check(query, message)
}

/// Whether `config_json` is a transformer that, applied to `message`, gives a transformed event. Exists so the processors (grok and
/// date patterns among them) can be fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_transformer(config_json: &str, message: &str) -> bool {
    transform::check(config_json, message)
}

/// Whether `message` is selected by `pattern`; `false` for a pattern that does not parse. Exists so the pattern parser can be
/// fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_filter_pattern(pattern: &str, message: &str) -> bool {
    pattern::parse(pattern).is_ok_and(|p| p.matches(message))
}

impl Service for Logs {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        Operation::from_index(op).is_some_and(dispatch::built)
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(self.ask(call))
    }

    fn streams(&self, op: u32) -> bool {
        Operation::from_index(op) == Some(Operation::StartLiveTail)
    }

    fn call_stream(&self, call: Call) -> BoxFuture<'_, Result<Download, ApiError>> {
        Box::pin(self.tail(call))
    }

    fn storage(&self) -> Option<&dyn aws_wire::storage::Storage> {
        Some(self)
    }
}
