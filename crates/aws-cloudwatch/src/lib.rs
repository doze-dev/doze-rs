//! CloudWatch: metrics, single-metric alarms, alarm history, dashboards and tags, as a state machine behind a lazy actor. State is on
//! disk and survives an idle reap and a restart; samples are only ever read from the database.
//!
//! It speaks the JSON 1.0 and the RPC v2 CBOR protocols (current SDKs prefer CBOR); the Query protocol is refused with an answer an
//! old client can read, as for SQS.
//!
//! **Alarms are evaluated at their deadlines, not on a tick.** After every call the service works out the earliest time at which any
//! alarm's state could change without new data (a period boundary at which an old datapoint leaves the window); one timer sleeps to
//! it, and when no alarm can change the service holds no timer. A write of data judges the alarms on that metric at once.
//!
//! Built: `PutMetricData`, `GetMetricStatistics`, `GetMetricData` (a single `MetricStat` per query), `ListMetrics`; alarms
//! (`PutMetricAlarm`, `PutCompositeAlarm`, describing, deleting, `SetAlarmState`, enabling and disabling actions, history, tags);
//! dashboards (the body kept verbatim); anomaly detectors (stored). Metric-math alarms and composite alarms are stored and returned;
//! they are not evaluated, and say so in their reason. An alarm action on an SNS topic or a Lambda function is carried out through
//! the peer bus when that service runs; otherwise the alarm's history says it was not delivered. Refused by name: everything else
//! (metric streams, Contributor Insights, the widget image, OTel enrichment, mute rules, resource metrics configuration).

mod actions;
mod alarms;
mod authz;
mod core;
mod docs;
mod eval;
mod model;
mod put;
mod read;
mod req;
mod stats;
mod timers;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use aws_model_cloudwatch::Operation;
use aws_model_cloudwatch::meta::META;
use aws_model_cloudwatch::tables::TABLES;
use aws_schema::json::Violation;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Caller, Reply, Service};
use doze_runtime::{Ctx, Keyed};

use crate::core::{Core, Request};
use crate::timers::Timers;

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// The CloudWatch service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Cloudwatch {
    /// One actor per region: a metric or alarm belongs to the region that made it.
    actors: Keyed<Core>,
    timers: Arc<Timers>,
    /// The service's directory, which `resume` looks through.
    dir: std::path::PathBuf,
}

impl Cloudwatch {
    /// Registers CloudWatch. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            dir: ctx.dir.clone(),
            timers: Timers::new(Arc::clone(&ctx)),
            actors: Keyed::new("cloudwatch", ctx, HOME_REGION),
        }
    }

    /// Starts the alarm timers of the regions that have CloudWatch state on disk. Run once when the process starts: nothing else
    /// would wake a region that nobody calls after a restart, and an alarm whose data aged out meanwhile would wait for a request.
    /// A region with no alarm to judge is opened and left to be reaped when it has been quiet.
    pub async fn resume(&self) {
        let mut regions = Vec::new();
        if self.dir.join(core::FILE).exists() {
            regions.push(HOME_REGION.to_owned());
        }
        if let Ok(entries) = std::fs::read_dir(self.dir.join("regions")) {
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
            let caller = Caller {
                access_key_id: "doze".to_owned(),
                account_id: String::new(),
                region: region.clone(),
                endpoint: String::new(),
                service: None,
                session_token: None,
            };
            if let Ok(mut answer) = actor.call(Request::Tick(caller.clone())).await {
                if let Some(saved) = answer.durable.take() {
                    // WHY: a failed save is the writer's to report; the next call finds the same state and says so.
                    drop(saved.await);
                }
                self.timers.arm(&region, &caller, &actor, answer.next_due);
            }
        }
    }

    /// Sends the call to the actor of the region it was signed for, holds the answer until what it changed is on disk, and tells the
    /// region's timer when its alarms next need judging.
    async fn ask(&self, call: Call) -> Reply {
        let region = call.caller.region.clone();
        let caller = call.caller.clone();
        let actor = match self.actors.actor(&region) {
            Ok(actor) => actor,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        let mut answer = match actor.call(Request::Call(call)).await {
            Ok(answer) => answer,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            return Err(ApiError::internal(format!("could not save: {e}")));
        }
        self.timers.arm(&region, &caller, &actor, answer.next_due);
        answer.reply
    }

    /// Whether the region has a running alarm timer (for tests).
    #[cfg(test)]
    pub(crate) fn timer_armed(&self, region: &str) -> bool {
        self.timers.armed(region)
    }
}

impl Service for Cloudwatch {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        Operation::from_index(op).is_some_and(core::built)
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(self.ask(call))
    }

    /// CloudWatch words a request that fails its model's checks as `ValidationError` (the Query code of `ValidationException`), or,
    /// for a bad `Unit`, as the `InvalidParameterValue` AWS recorded (2026-11-09).
    fn invalid(&self, violation: &Violation) -> ApiError {
        let mut e = aws_wire::service::default_invalid(&TABLES, violation);
        match violation {
            Violation::Enum { member, .. }
                if TABLES.member(*member).is_some_and(|m| m.name == "Unit") =>
            {
                // The index of the data point is not in the violation; the recorded request had one, so the message says member 1.
                return ApiError::modeled("InvalidParameterValueException", put::UNIT_MESSAGE);
            }
            Violation::Enum { member, value }
                if TABLES
                    .member(*member)
                    .is_some_and(|m| m.name == "ComparisonOperator") =>
            {
                // The order AWS listed the set in (recorded 2026-11-09); it is not the model's, nor its reverse.
                return ApiError::modeled(
                    "ValidationException",
                    format!(
                        "1 validation error detected: Value '{}' at 'comparisonOperator' failed to satisfy constraint: Member must satisfy enum value set: [GreaterThanUpperThreshold, LessThanLowerThreshold, GreaterThanThreshold, LessThanThreshold, LessThanOrEqualToThreshold, LessThanLowerOrGreaterThanUpperThreshold, GreaterThanOrEqualToThreshold]",
                        value.as_deref().unwrap_or("null")
                    ),
                );
            }
            Violation::Missing { member, .. } => {
                let name = TABLES.member(*member).map_or("", |m| m.name);
                return ApiError::modeled(
                    "MissingRequiredParameterException",
                    format!("The request must contain the parameter {name}"),
                );
            }
            _ => {}
        }
        if e.code == "ValidationException" {
            e = ApiError::modeled("ValidationException", e.message);
        }
        e
    }
}
