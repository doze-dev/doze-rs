//! The state machine without a socket: requests are decoded against the model and dispatched on a core with a manual clock.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_testkit::{Harness, TempDir};
use doze_runtime::{Ctx, ManualClock, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Logs;
use crate::core::Core;

mod events;
mod groups;
mod persistence;

pub(crate) const NOW: i64 = 1_700_000_000_000;

pub(crate) struct World {
    pub(crate) core: Core,
    pub(crate) clock: Arc<ManualClock>,
    h: Harness<Logs>,
}

impl World {
    pub(crate) fn new() -> Self {
        let clock = Arc::new(ManualClock::at(NOW));
        Self::with(Core::in_memory(clock.clone(), None), clock)
    }

    pub(crate) fn with(core: Core, clock: Arc<ManualClock>) -> Self {
        let ctx = Ctx {
            dir: std::env::temp_dir().join("doze-aws-logs-unused"),
            idle: std::time::Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        };
        Self {
            core,
            clock,
            h: Harness::new(Logs::new(Arc::new(ctx))),
        }
    }

    /// Runs one operation: the request as JSON in, the answer as JSON (or the error) out.
    pub(crate) fn run(&mut self, op: &str, request: &Value) -> Result<Value, aws_wire::ApiError> {
        self.run_as(None, op, request)
    }

    /// Runs one operation as another service acting for the caller (Lambda writing a function's log).
    pub(crate) fn run_as(
        &mut self,
        service: Option<&str>,
        op: &str,
        request: &Value,
    ) -> Result<Value, aws_wire::ApiError> {
        let ix = self.h.op(op);
        let mut call = self.h.call(ix, request)?;
        call.caller.service = service.map(str::to_owned);
        let out = self.core.dispatch(&call);
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

    pub(crate) fn group(&mut self, name: &str) {
        self.ok("CreateLogGroup", &json!({"logGroupName": name}));
    }

    pub(crate) fn stream(&mut self, group: &str, stream: &str) {
        self.ok(
            "CreateLogStream",
            &json!({"logGroupName": group, "logStreamName": stream}),
        );
    }

    /// Puts messages at consecutive milliseconds from `at`.
    pub(crate) fn put(&mut self, group: &str, stream: &str, at: i64, messages: &[&str]) -> Value {
        let events: Vec<Value> = messages
            .iter()
            .enumerate()
            .map(|(i, m)| json!({"timestamp": at + i64::try_from(i).unwrap(), "message": m}))
            .collect();
        self.ok(
            "PutLogEvents",
            &json!({"logGroupName": group, "logStreamName": stream, "logEvents": events}),
        )
    }
}

#[test]
fn what_is_not_built_is_refused_by_name() {
    let w = World::new();
    for op in ["PutLogEvents", "FilterLogEvents", "PutMetricFilter"] {
        assert!(w.h.serves(op), "{op} is built");
    }
    for op in [
        "StartQuery",
        "StartLiveTail",
        "CreateExportTask",
        "PutDestination",
        "CreateDelivery",
    ] {
        assert!(!w.h.serves(op), "{op} is refused");
    }
}

pub(crate) fn temp() -> TempDir {
    TempDir::new("logs")
}
