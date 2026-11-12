//! `AWS/Lambda` metrics: `Invocations`, `Errors`, `Throttles` and `Duration` for every invocation, as AWS publishes them, by
//! `FunctionName`, by `FunctionName` and `Resource` (the function, or `function:qualifier` when a version or alias was invoked), by
//! those and `ExecutedVersion` when an alias was invoked, and across all functions (no dimensions).
//!
//! **Held until read.** An invocation adds to a per-minute count in memory and costs nothing more: nothing is sent while nothing
//! reads the namespace. CloudWatch takes what is held before it answers a read (`ListMetrics`, `GetMetricStatistics`,
//! `GetMetricData`, `DescribeAlarms`, `PutMetricAlarm`) through the producer hook on the bus; while a metric alarm in the region
//! watches `AWS/Lambda`, each invocation sends what is held at once, so the alarm judges it as it comes. What is held is lost with
//! the process (a bounded ring of [`MAX_SLOTS`] minute counts; the oldest go first).
//!
//! From AWS's documentation and memory, not a recording: the dimension sets, `Errors` as a 0 or 1 per invocation (so its average is
//! the error rate), `Throttles` counted apart from `Invocations`, `Resource` naming an unqualified or `$LATEST` invocation by the
//! function's name alone. Not built: `ConcurrentExecutions`, `IteratorAge`, the async and event source mapping metrics.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use aws_peers::{Made, Peers};
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::Inner;
use crate::resolve::Resolved;

/// The namespace.
const NAMESPACE: &str = "AWS/Lambda";
/// Minute counts held at most (one per minute, region, account and dimension set).
pub(crate) const MAX_SLOTS: usize = 4096;
/// Durations kept per minute count, for percentiles; past it the counts go on and durations are not added.
const MAX_DURATIONS: usize = 10_000;
/// Values one data point carries.
const VALUES_PER_POINT: usize = 150;

/// What an invocation is counted under.
#[derive(Debug, Clone, Default)]
pub(crate) struct Target {
    region: String,
    account: String,
    function: String,
    /// `function:qualifier` for a version or alias, otherwise the function.
    resource: String,
    /// The version run, when an alias was invoked.
    executed: Option<String>,
}

impl Target {
    pub(crate) fn of(rs: &Resolved, caller: &Caller) -> Self {
        let qualifier = rs
            .invoked_arn
            .splitn(8, ':')
            .nth(7)
            .filter(|q| !q.is_empty() && *q != "$LATEST");
        let alias = qualifier.filter(|q| q.parse::<u64>().is_err());
        Self {
            region: rs.region.clone(),
            account: caller.account_id.clone(),
            function: rs.name.clone(),
            resource: qualifier.map_or_else(|| rs.name.clone(), |q| format!("{}:{q}", rs.name)),
            executed: alias.map(|_| rs.version.clone()),
        }
    }

    /// The dimension sets one invocation counts in.
    fn sets(&self) -> Vec<Vec<(String, String)>> {
        let f = ("FunctionName".to_owned(), self.function.clone());
        let r = ("Resource".to_owned(), self.resource.clone());
        let mut out = vec![Vec::new(), vec![f.clone()], vec![f.clone(), r.clone()]];
        if let Some(v) = &self.executed {
            out.push(vec![f, r, ("ExecutedVersion".to_owned(), v.clone())]);
        }
        out
    }
}

/// One minute of one dimension set.
#[derive(Debug, Default)]
struct Slot {
    invocations: u64,
    errors: u64,
    throttles: u64,
    durations: Vec<f64>,
}

/// Minute, region, account, dimensions: the oldest minute sorts first.
type Key = (i64, String, String, Vec<(String, String)>);

/// The counts held until CloudWatch takes them.
#[derive(Debug, Default)]
pub(crate) struct Meter {
    slots: Mutex<BTreeMap<Key, Slot>>,
}

/// How one invocation ended, for the counts.
pub(crate) enum Outcome {
    Ran {
        duration_ms: f64,
        error: bool,
    },
    /// It failed before it began (the process could not start): an invocation and an error, with no duration.
    Failed,
    Throttled,
}

impl Outcome {
    /// An invocation that did not begin: throttled (reserved concurrency used up), or failed to start.
    pub(crate) fn not_started(throttled: bool) -> Self {
        if throttled {
            Self::Throttled
        } else {
            Self::Failed
        }
    }
}

impl Meter {
    /// A meter whose counts CloudWatch takes through `peers`, when there is a bus.
    pub(crate) fn install(peers: Option<&Arc<Peers>>) -> Arc<Self> {
        let meter = Arc::new(Self::default());
        if let Some(p) = peers {
            let m = Arc::clone(&meter);
            p.add_producer(Arc::new(move |region: &str| m.take(region)));
        }
        meter
    }

    pub(crate) fn add(&self, t: &Target, at_ms: i64, outcome: &Outcome) {
        let minute = at_ms - at_ms.rem_euclid(60_000);
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        for dims in t.sets() {
            let s = slots
                .entry((minute, t.region.clone(), t.account.clone(), dims))
                .or_default();
            match outcome {
                Outcome::Ran { duration_ms, error } => {
                    s.invocations += 1;
                    s.errors += u64::from(*error);
                    if s.durations.len() < MAX_DURATIONS {
                        s.durations.push(*duration_ms);
                    }
                }
                Outcome::Failed => {
                    s.invocations += 1;
                    s.errors += 1;
                }
                Outcome::Throttled => s.throttles += 1,
            }
        }
        while slots.len() > MAX_SLOTS {
            if slots.pop_first().is_none() {
                break;
            }
        }
    }

    /// Hands over, and forgets, what is held for `region`: one `PutMetricData` request per account, of at most 1,000 data points.
    pub(crate) fn take(&self, region: &str) -> Vec<Made> {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let keys: Vec<Key> = slots.keys().filter(|k| k.1 == region).cloned().collect();
        let mut by_account: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for key in keys {
            let Some(slot) = slots.remove(&key) else {
                continue;
            };
            let (minute, _, account, dims) = key;
            by_account
                .entry(account)
                .or_default()
                .extend(points(minute, &dims, &slot));
        }
        drop(slots);
        let mut out = Vec::new();
        for (account, data) in by_account {
            for chunk in data.chunks(1000) {
                out.push(Made {
                    service: "lambda",
                    account: account.clone(),
                    body: json!({"Namespace": NAMESPACE, "MetricData": chunk}),
                });
            }
        }
        out
    }
}

/// The data points of one minute count.
fn points(minute: i64, dims: &[(String, String)], s: &Slot) -> Vec<Value> {
    let dims: Vec<Value> = dims
        .iter()
        .map(|(n, v)| json!({"Name": n, "Value": v}))
        .collect();
    #[allow(clippy::cast_precision_loss)]
    let at = minute as f64 / 1000.0;
    let point = |name: &str, unit: &str, values: &[(f64, u64)]| {
        let mut d = json!({
            "MetricName": name,
            "Timestamp": at,
            "Unit": unit,
            "Values": values.iter().map(|(v, _)| *v).collect::<Vec<_>>(),
            "Counts": values.iter().map(|(_, c)| *c).collect::<Vec<_>>(),
        });
        if let (Some(o), false) = (d.as_object_mut(), dims.is_empty()) {
            o.insert("Dimensions".into(), Value::Array(dims.clone()));
        }
        d
    };
    let mut out = Vec::new();
    if s.invocations > 0 {
        out.push(point("Invocations", "Count", &[(1.0, s.invocations)]));
        let fine = s.invocations - s.errors.min(s.invocations);
        let errors: Vec<(f64, u64)> = [(0.0, fine), (1.0, s.errors)]
            .into_iter()
            .filter(|(_, c)| *c > 0)
            .collect();
        out.push(point("Errors", "Count", &errors));
    }
    if s.throttles > 0 {
        out.push(point("Throttles", "Count", &[(1.0, s.throttles)]));
    }
    let mut durations: BTreeMap<u64, u64> = BTreeMap::new();
    for d in &s.durations {
        *durations.entry(d.to_bits()).or_default() += 1;
    }
    let durations: Vec<(f64, u64)> = durations
        .into_iter()
        .map(|(bits, c)| (f64::from_bits(bits), c))
        .collect();
    for chunk in durations.chunks(VALUES_PER_POINT) {
        out.push(point("Duration", "Milliseconds", chunk));
    }
    out
}

impl Inner {
    /// Counts an invocation, and sends what is held at once when an alarm in the region watches `AWS/Lambda`.
    pub(crate) async fn count_invocation(&self, t: &Target, outcome: Outcome) {
        self.meter.add(t, self.clock.now_ms(), &outcome);
        let Some(peers) = self.peers.clone() else {
            return;
        };
        if !peers.watched(&t.region, NAMESPACE) {
            return;
        }
        for made in self.meter.take(&t.region) {
            let caller = Caller {
                access_key_id: "lambda".to_owned(),
                account_id: made.account,
                region: t.region.clone(),
                endpoint: String::new(),
                service: None,
                session_token: None,
            };
            if let Err(e) = peers
                .call(
                    made.service,
                    &caller,
                    "monitoring",
                    "PutMetricData",
                    &made.body,
                )
                .await
            {
                tracing::debug!(error = %e.message, "lambda: AWS/Lambda metrics were not taken");
            }
        }
    }
}

#[cfg(test)]
mod tests;
