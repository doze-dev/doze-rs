//! The state machine without a socket: requests are decoded against the model and dispatched on a core with a manual clock; answers
//! are read back through the model's output shapes. SQS runs for real beside it on the peer bus, so a delivery is read back.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_model_scheduler::tables::TABLES;
use aws_peers::Peers;
use aws_schema::patterns::Patterns;
use aws_testkit::{Harness, TempDir};
use aws_wire::service::Caller;
use doze_runtime::{Ctx, ManualClock, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::core::{Core, Request};
use crate::{Scheduler, Shared};

mod delivery;
mod firing;
mod groups;
mod schedules;
mod service;

/// 2026-01-01 00:00:00 UTC.
pub(crate) const START: i64 = 1_767_225_600_000;
pub(crate) const ACCOUNT: &str = "000000000000";
pub(crate) const ROLE: &str = "arn:aws:iam::000000000000:role/scheduler";

pub(crate) fn ctx(
    dir: &std::path::Path,
    ext: Option<Arc<dyn std::any::Any + Send + Sync>>,
) -> Arc<Ctx> {
    Arc::new(Ctx {
        dir: dir.to_path_buf(),
        idle: std::time::Duration::from_secs(300),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext,
    })
}

pub(crate) fn caller() -> Caller {
    Caller {
        access_key_id: "test".into(),
        account_id: ACCOUNT.into(),
        region: "us-east-1".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: None,
    }
}

/// A region of Scheduler, and SQS beside it on the bus, on a runtime the test drives.
pub(crate) struct World {
    pub(crate) core: Core,
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) peers: Arc<Peers>,
    pub(crate) rt: tokio::runtime::Runtime,
    h: Harness<Scheduler>,
    patterns: Patterns,
    _dir: TempDir,
}

impl World {
    pub(crate) fn new() -> Self {
        let dir = TempDir::new("scheduler");
        let peers = Peers::new();
        peers.set_mode(aws_peers::Mode::Off);
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        peers.register(Arc::new(aws_sqs::Sqs::new(ctx(
            &dir.path().join("sqs"),
            Some(ext),
        ))));
        let clock = Arc::new(ManualClock::at(START));
        let core = Core::in_memory(clock.clone(), Some(peers.clone()));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let h = Harness::new(Scheduler::new(ctx(&dir.path().join("unused"), None)));
        Self {
            core,
            clock,
            peers,
            rt,
            h,
            patterns: Patterns::new(&TABLES),
            _dir: dir,
        }
    }

    pub(crate) fn run(&mut self, op: &str, request: &Value) -> Result<Value, aws_wire::ApiError> {
        let ix = self.h.op(op);
        let call = self.h.call(ix, request)?;
        let value = self.core.dispatch(&call)?;
        let out = aws_wire::handler::reply_json(&TABLES, &self.patterns, ix, &value)?;
        Ok(self.h.json(ix, &out))
    }

    pub(crate) fn ok(&mut self, op: &str, request: &Value) -> Value {
        self.run(op, request)
            .unwrap_or_else(|e| panic!("{op}: {} {}", e.code, e.message))
    }

    /// The error's code and message.
    pub(crate) fn err(&mut self, op: &str, request: &Value) -> (String, String) {
        let e = self.run(op, request).unwrap_err();
        (e.code.to_string(), e.message)
    }

    /// The actor's wake-up: what it fired.
    pub(crate) fn tick(&mut self) -> Vec<crate::fire::Invocation> {
        let answer = self
            .rt
            .block_on(doze_runtime::Service::handle(&mut self.core, Request::Tick));
        answer.fired
    }

    pub(crate) fn shared(&self) -> Arc<Shared> {
        Arc::new(Shared {
            peers: Some(self.peers.clone()),
            clock: self.clock.clone(),
            tasks: Tasks::new(),
            sessions: crate::role::Sessions::default(),
        })
    }

    /// Makes a queue and answers its URL and ARN.
    pub(crate) fn queue(&self, name: &str, attrs: Value) -> (String, String) {
        let made = self
            .rt
            .block_on(self.peers.call(
                "test",
                &caller(),
                "sqs",
                "CreateQueue",
                &json!({"QueueName": name, "Attributes": attrs}),
            ))
            .unwrap();
        let url = made["QueueUrl"].as_str().unwrap().to_owned();
        (url, format!("arn:aws:sqs:us-east-1:{ACCOUNT}:{name}"))
    }

    /// The messages waiting in a queue (with their attributes).
    pub(crate) fn receive(&self, url: &str) -> Vec<Value> {
        let got = self
            .rt
            .block_on(self.peers.call(
                "test",
                &caller(),
                "sqs",
                "ReceiveMessage",
                &json!({"QueueUrl": url, "MaxNumberOfMessages": 10, "MessageAttributeNames": ["All"]}),
            ))
            .unwrap();
        got.get("Messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }
}

/// A schedule request with a target on `arn`.
pub(crate) fn schedule(name: &str, expr: &str, arn: &str) -> Value {
    json!({
        "Name": name,
        "ScheduleExpression": expr,
        "FlexibleTimeWindow": {"Mode": "OFF"},
        "Target": {"Arn": arn, "RoleArn": ROLE, "Input": "hello"},
    })
}
