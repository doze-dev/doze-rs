//! The state machine without a socket: requests are decoded against the model and dispatched on a core with a manual clock, and the
//! services it delivers to (SQS, SNS, Kinesis, Logs) run for real beside it on the peer bus, so a delivery is read back.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_peers::Peers;
use aws_testkit::{Harness, TempDir};
use aws_wire::service::Caller;
use doze_runtime::{Ctx, ManualClock, Service, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Eventbridge;
use crate::core::{Core, Request};

mod archives;
mod buses;
mod connections;
mod delivery;
mod refusals;
mod rules;
mod schedules;
mod services;

/// 2023-11-14 22:13:20 UTC, on a minute boundary.
pub(crate) const START: i64 = 1_699_999_980_000;
pub(crate) const ACCOUNT: &str = "000000000000";

fn ctx(dir: &std::path::Path, ext: Option<Arc<dyn std::any::Any + Send + Sync>>) -> Arc<Ctx> {
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

/// A region of EventBridge, and the services it delivers to beside it on the bus, on a runtime the test drives.
pub(crate) struct World {
    pub(crate) core: Core,
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) peers: Arc<Peers>,
    pub(crate) rt: tokio::runtime::Runtime,
    h: Harness<Eventbridge>,
    _dir: TempDir,
}

impl World {
    pub(crate) fn new() -> Self {
        let dir = TempDir::new("eventbridge");
        let peers = Peers::new();
        // As the binary starts: policies are asked, and a refusal is logged and goes through.
        peers.set_mode(aws_peers::Mode::Soft);
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        let sub = |name: &str| ctx(&dir.path().join(name), Some(ext.clone()));
        peers.register(Arc::new(aws_sqs::Sqs::new(sub("sqs"))));
        peers.register(Arc::new(aws_sns::Sns::new(sub("sns"))));
        peers.register(Arc::new(aws_kinesis::Kinesis::new(sub("kinesis"))));
        peers.register(Arc::new(aws_logs::Logs::new(sub("logs"))));
        let clock = Arc::new(ManualClock::at(START));
        let core = Core::in_memory(clock.clone(), Some(peers.clone()));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let h = Harness::new(Eventbridge::new(ctx(
            &std::env::temp_dir().join("doze-aws-eventbridge-unused"),
            None,
        )));
        Self {
            core,
            clock,
            peers,
            rt,
            h,
            _dir: dir,
        }
    }

    pub(crate) fn run(&mut self, op: &str, request: &Value) -> Result<Value, aws_wire::ApiError> {
        let ix = self.h.op(op);
        let call = self.h.call(ix, request)?;
        let out = self.rt.block_on(self.core.dispatch(&call));
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

    pub(crate) fn served(&self, op: &str) -> bool {
        self.h.serves(op)
    }

    /// A call as the actor takes it: authorized first, then run.
    pub(crate) fn handled(
        &mut self,
        op: &str,
        request: &Value,
    ) -> Result<Value, aws_wire::ApiError> {
        let ix = self.h.op(op);
        let call = self.h.call(ix, request)?;
        let answer = self.rt.block_on(self.core.handle(Request::Call(call)));
        Ok(self.h.json(ix, &answer.reply?))
    }

    /// The actor's wake-up at `deadline`, and the next deadline it asks for.
    pub(crate) fn tick(&mut self, deadline: i64) -> Option<i64> {
        let answer = self
            .rt
            .block_on(self.core.handle(Request::Tick(caller(), deadline)));
        if let Some(saved) = self.core.flush().unwrap() {
            saved.wait().unwrap();
        }
        answer.next_due
    }

    /// Moves the clock to `ms` and wakes the actor as its timer would.
    pub(crate) fn wake_at(&mut self, ms: i64) -> Option<i64> {
        self.clock.set(ms);
        self.tick(ms)
    }

    /// A call to another service on the bus, as the account.
    pub(crate) fn peer(
        &self,
        service: &str,
        op: &str,
        body: &Value,
    ) -> Result<Value, aws_wire::ApiError> {
        self.rt
            .block_on(self.peers.call("test", &caller(), service, op, body))
    }

    /// Makes a queue that the rule may deliver to; returns its ARN.
    pub(crate) fn queue(&self, name: &str, rule_arn: &str) -> String {
        let made = self
            .peer("sqs", "CreateQueue", &json!({"QueueName": name}))
            .unwrap();
        let attrs = self
            .peer(
                "sqs",
                "GetQueueAttributes",
                &json!({"QueueUrl": made["QueueUrl"], "AttributeNames": ["QueueArn"]}),
            )
            .unwrap();
        let arn = attrs["Attributes"]["QueueArn"].as_str().unwrap().to_owned();
        let policy = allow_events(&arn, rule_arn);
        self.peer(
            "sqs",
            "SetQueueAttributes",
            &json!({"QueueUrl": made["QueueUrl"], "Attributes": {"Policy": policy}}),
        )
        .unwrap();
        arn
    }

    pub(crate) fn queue_url(name: &str) -> String {
        format!("http://localhost:4566/{ACCOUNT}/{name}")
    }

    /// Everything waiting in a queue.
    pub(crate) fn drain(&self, name: &str) -> Vec<Value> {
        let got = self
            .peer(
                "sqs",
                "ReceiveMessage",
                &json!({"QueueUrl": Self::queue_url(name), "MaxNumberOfMessages": 10,
                    "MessageAttributeNames": ["All"], "MessageSystemAttributeNames": ["All"]}),
            )
            .unwrap();
        got["Messages"].as_array().cloned().unwrap_or_default()
    }

    /// The bodies waiting in a queue, read as JSON.
    pub(crate) fn bodies(&self, name: &str) -> Vec<Value> {
        self.drain(name)
            .iter()
            .map(|m| serde_json::from_str(m["Body"].as_str().unwrap()).unwrap())
            .collect()
    }

    /// A bus, made; returns its name.
    pub(crate) fn bus(&mut self, name: &str) -> String {
        self.ok("CreateEventBus", &json!({"Name": name}));
        name.to_owned()
    }

    /// A rule with a pattern on a bus; returns its ARN.
    pub(crate) fn rule(&mut self, bus: &str, name: &str, pattern: &Value) -> String {
        self.ok(
            "PutRule",
            &json!({"Name": name, "EventBusName": bus, "EventPattern": pattern.to_string()}),
        )["RuleArn"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Puts one event on a bus; returns the answer.
    pub(crate) fn put(&mut self, bus: &str, source: &str, detail: &Value) -> Value {
        self.ok(
            "PutEvents",
            &json!({"Entries": [{"EventBusName": bus, "Source": source, "DetailType": "x", "Detail": detail.to_string()}]}),
        )
    }
}

/// A queue policy that lets the rule deliver to the queue.
pub(crate) fn allow_events(queue_arn: &str, rule_arn: &str) -> String {
    json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": {"Service": "events.amazonaws.com"},
            "Action": "sqs:SendMessage",
            "Resource": queue_arn,
            "Condition": {"ArnEquals": {"aws:SourceArn": rule_arn}}
        }]
    })
    .to_string()
}
