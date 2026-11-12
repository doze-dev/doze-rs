//! The state a CloudWatch service owns, and the dispatch from an operation to the code that does it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use aws_model_cloudwatch::Operation;
use aws_model_cloudwatch::tables::TABLES;
use aws_peers::{Mode, Peers};
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply_json};
use aws_wire::service::{Call, Caller, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::rows::RowTable;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::model::{Alarm, Doc, History, Series};
use crate::req::Req;

const FILE: &str = "cloudwatch.redb";
const SERIES: &str = "series";
const ALARMS: &str = "alarms";
const HISTORY: &str = "history";
const DOCS: &str = "docs";
const SAMPLES: &str = "samples";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
pub(crate) const VERSION: u8 = 1;

/// What the actor is asked: a call from the wire, or a wake-up at an alarm's deadline.
#[derive(Debug)]
pub(crate) enum Request {
    Call(Call),
    /// Evaluate the alarms that are due. The caller is whoever armed the timer: it says the account and region the alarms are in.
    Tick(Caller),
}

/// An alarm action to carry out once the call that caused it has been answered.
#[derive(Debug)]
pub(crate) struct Outgoing {
    pub(crate) alarm: String,
    /// The ARN to act on.
    pub(crate) action: String,
    pub(crate) old: String,
    pub(crate) new: String,
    pub(crate) at_ms: i64,
}

/// All of CloudWatch in one region. Series, alarms, history and documents are held in memory; samples are only ever read from the
/// database.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) series: Records<Series>,
    pub(crate) alarms: Records<Alarm>,
    pub(crate) history: Records<History>,
    pub(crate) docs: Records<Doc>,
    pub(crate) samples: RowTable,
    pub(crate) db: Option<Db>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) patterns: Patterns,
    /// When each evaluated alarm next needs looking at: its state can change at that time and not before, unless data arrives.
    pub(crate) due: HashMap<String, i64>,
    /// Whether `due` has been worked out since the service opened.
    pub(crate) due_ready: bool,
    /// Alarm actions waiting to be carried out.
    pub(crate) outbox: Vec<Outgoing>,
    /// When old samples were last removed.
    pub(crate) trimmed_ms: i64,
    /// Metrics written by the call in progress that an alarm watches.
    pub(crate) to_evaluate: Vec<String>,
}

/// What the actor answers.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
    /// The earliest time any alarm needs evaluating, if one does.
    pub(crate) next_due: Option<i64>,
}

/// The operations this service runs. Every other is refused by name before its request is read.
pub(crate) fn built(op: Operation) -> bool {
    use Operation as O;
    matches!(
        op,
        O::PutMetricData
            | O::ListMetrics
            | O::GetMetricStatistics
            | O::GetMetricData
            | O::PutMetricAlarm
            | O::PutCompositeAlarm
            | O::DescribeAlarms
            | O::DescribeAlarmsForMetric
            | O::DescribeAlarmHistory
            | O::DeleteAlarms
            | O::SetAlarmState
            | O::EnableAlarmActions
            | O::DisableAlarmActions
            | O::TagResource
            | O::UntagResource
            | O::ListTagsForResource
            | O::PutDashboard
            | O::GetDashboard
            | O::ListDashboards
            | O::DeleteDashboards
            | O::PutAnomalyDetector
            | O::DescribeAnomalyDetectors
            | O::DeleteAnomalyDetector
    )
}

impl doze_runtime::Service for Core {
    type Request = Request;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))
    }

    async fn handle(&mut self, req: Request) -> Answer {
        self.ensure_due();
        let (caller, mut reply) = match req {
            Request::Call(call) => {
                if let Some(refusal) = self.refused(&call).await {
                    return Answer {
                        reply: Err(refusal),
                        durable: None,
                        next_due: self.earliest_due(),
                    };
                }
                let mut reply = self.dispatch(&call);
                if let Err(e) = self.settle().await {
                    reply = Err(e);
                }
                (call.caller, reply)
            }
            Request::Tick(caller) => {
                self.tick();
                (caller, empty())
            }
        };
        self.deliver(&caller).await;
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        Answer {
            reply,
            durable,
            next_due: self.earliest_due(),
        }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    /// CloudWatch with no disk (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>, peers: Option<Arc<Peers>>) -> Self {
        Self::with(
            clock,
            (
                Records::new(SERIES, "metric", VERSION),
                Records::new(ALARMS, "alarm", VERSION),
                Records::new(HISTORY, "alarm history", VERSION),
                Records::new(DOCS, "document", VERSION),
            ),
            RowTable::new(SAMPLES, true),
            None,
            peers,
        )
    }

    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[SERIES, ALARMS, HISTORY, DOCS, SAMPLES])?;
        let read = db.read()?;
        let loaded = (
            Records::load(SERIES, "metric", VERSION, &read)?,
            Records::load(ALARMS, "alarm", VERSION, &read)?,
            Records::load(HISTORY, "alarm history", VERSION, &read)?,
            Records::load(DOCS, "document", VERSION, &read)?,
        );
        drop(read);
        Ok(Self::with(
            clock,
            loaded,
            RowTable::new(SAMPLES, false),
            Some(db),
            peers,
        ))
    }

    fn with(
        clock: Arc<dyn Clock>,
        (series, alarms, history, docs): (
            Records<Series>,
            Records<Alarm>,
            Records<History>,
            Records<Doc>,
        ),
        samples: RowTable,
        db: Option<Db>,
        peers: Option<Arc<Peers>>,
    ) -> Self {
        Self {
            series,
            alarms,
            history,
            docs,
            samples,
            db,
            clock,
            peers,
            patterns: Patterns::new(&TABLES),
            due: HashMap::new(),
            due_ready: false,
            outbox: Vec::new(),
            trimmed_ms: 0,
            to_evaluate: Vec::new(),
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// When the call wrote data an alarm watches: waits until it is on disk (a read of the samples finds only what is committed), then
    /// judges the alarms. A write nothing watches is not waited for.
    async fn settle(&mut self) -> Result<(), ApiError> {
        if self.to_evaluate.is_empty() {
            return Ok(());
        }
        let saved = self
            .flush()
            .map_err(|e| ApiError::internal(format!("could not save: {e}")))?;
        if let Some(ack) = saved {
            ack.await
                .map_err(|e| ApiError::internal(format!("could not save: {e}")))?;
        }
        self.evaluate_pending();
        Ok(())
    }

    /// Asks the authorizer, when there is one. `Some` is the refusal.
    async fn refused(&self, call: &Call) -> Option<ApiError> {
        let peers = self.peers.clone()?;
        if peers.mode() == Mode::Off {
            return None;
        }
        peers
            .authorize(&call.caller, crate::authz::needs(call))
            .await
            .err()
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.series.stage()?,
            self.alarms.stage()?,
            self.history.stage()?,
            self.docs.stage()?,
        ];
        let rows = vec![self.samples.stage()];
        Ok(self
            .db
            .as_ref()
            .and_then(|db| db.submit_with_rows(changes, rows)))
    }

    /// Runs the call and writes the answer as the operation's output.
    pub(crate) fn dispatch(&mut self, call: &Call) -> Reply {
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(not_built());
        };
        let r = Req::new(call);
        let value = self.run(op, &r)?;
        reply_json(&TABLES, &self.patterns, call.op, &value)
    }

    fn run(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::PutMetricData => self.put_metric_data(r),
            O::ListMetrics => self.list_metrics(r),
            O::GetMetricStatistics => self.get_metric_statistics(r),
            O::GetMetricData => self.get_metric_data(r),
            O::PutMetricAlarm => self.put_metric_alarm(r),
            O::PutCompositeAlarm => self.put_composite_alarm(r),
            O::DescribeAlarms => self.describe_alarms(r),
            O::DescribeAlarmsForMetric => self.describe_alarms_for_metric(r),
            O::DescribeAlarmHistory => self.describe_alarm_history(r),
            O::DeleteAlarms => Ok(self.delete_alarms(r)),
            O::SetAlarmState => self.set_alarm_state(r),
            O::EnableAlarmActions => Ok(self.alarm_actions_enabled(r, true)),
            O::DisableAlarmActions => Ok(self.alarm_actions_enabled(r, false)),
            O::TagResource => self.tag(r),
            O::UntagResource => self.untag(r),
            O::ListTagsForResource => self.list_tags(r),
            other => self.run_docs(other, r),
        }
    }
}

/// An operation that is refused by name.
pub(crate) fn not_built() -> ApiError {
    ApiError::new(
        501,
        "NotImplemented",
        "This operation is not built in doze-aws.",
    )
}

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
}
