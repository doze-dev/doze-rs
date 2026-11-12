//! The state machine without a socket: requests are decoded against the model and dispatched on a core with a manual clock.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_testkit::{Harness, TempDir};
use doze_runtime::{Ctx, ManualClock, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Cloudwatch;
use crate::core::Core;

mod alarms;
mod cbor;
mod metrics;
mod producers;
mod timer;

/// 2023-11-14 22:13:20 UTC, a Tuesday, on a minute boundary.
pub(crate) const NOW: i64 = 1_699_999_980_000;

pub(crate) struct World {
    pub(crate) core: Core,
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) h: Harness<Cloudwatch>,
}

impl World {
    pub(crate) fn new() -> Self {
        let clock = Arc::new(ManualClock::at(NOW));
        Self::with(Core::in_memory(clock.clone(), None), clock)
    }

    pub(crate) fn with(core: Core, clock: Arc<ManualClock>) -> Self {
        let ctx = Ctx {
            dir: std::env::temp_dir().join("doze-aws-cloudwatch-unused"),
            idle: std::time::Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        };
        Self {
            core,
            clock,
            h: Harness::new(Cloudwatch::new(Arc::new(ctx))),
        }
    }

    /// Runs one operation: the request as JSON in, the answer as JSON (or the error) out.
    pub(crate) fn run(&mut self, op: &str, request: &Value) -> Result<Value, aws_wire::ApiError> {
        let ix = self.h.op(op);
        let call = self.h.call(ix, request)?;
        let out = self.core.dispatch(&call);
        // What `handle` does once the data is committed (with no disk the data is there already).
        self.core.evaluate_pending();
        if let Some(saved) = self.core.flush().unwrap() {
            saved.wait().unwrap();
        }
        Ok(self.h.json(ix, &out?))
    }

    pub(crate) fn ok(&mut self, op: &str, request: &Value) -> Value {
        self.run(op, request)
            .unwrap_or_else(|e| panic!("{op}: {} {}", e.code, e.message))
    }

    /// The error's code (its shape's name when it has one), and its message.
    pub(crate) fn err(&mut self, op: &str, request: &Value) -> (String, String) {
        let e = self.run(op, request).unwrap_err();
        (
            e.shape
                .map_or_else(|| e.code.to_string(), |s| s.to_string()),
            e.message,
        )
    }

    /// Puts a value for a metric of this test's namespace at `at` ms.
    pub(crate) fn put(&mut self, metric: &str, at: i64, value: f64) {
        self.ok(
            "PutMetricData",
            &json!({"Namespace": "T", "MetricData": [{
                "MetricName": metric, "Timestamp": at as f64 / 1000.0, "Value": value, "Unit": "Count"}]}),
        );
    }

    pub(crate) fn alarm(&mut self, name: &str, extra: &Value) {
        let mut req = json!({
            "AlarmName": name, "Namespace": "T", "MetricName": "m", "Statistic": "Maximum", "Period": 60,
            "EvaluationPeriods": 1, "Threshold": 100.0, "ComparisonOperator": "GreaterThanThreshold",
        });
        for (k, v) in extra.as_object().unwrap() {
            req[k] = v.clone();
        }
        self.ok("PutMetricAlarm", &req);
    }

    pub(crate) fn state(&mut self, name: &str) -> String {
        let d = self.ok("DescribeAlarms", &json!({"AlarmNames": [name]}));
        d["MetricAlarms"][0]["StateValue"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

#[test]
fn what_is_not_built_is_refused_by_name() {
    let w = World::new();
    for op in [
        "PutMetricData",
        "SetAlarmState",
        "PutDashboard",
        "GetMetricData",
    ] {
        assert!(w.h.serves(op), "{op} is built");
    }
    for op in [
        "PutMetricStream",
        "GetInsightRuleReport",
        "GetMetricWidgetImage",
        "PutAlarmMuteRule",
        "StartOTelEnrichment",
    ] {
        assert!(!w.h.serves(op), "{op} is refused");
    }
}

pub(crate) fn temp() -> TempDir {
    TempDir::new("cloudwatch")
}
