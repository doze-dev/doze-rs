//! The state machine without a socket: requests are decoded against the model and dispatched on a core with a manual clock, and SQS
//! runs for real beside it on the peer bus, so a delivery is read back from a queue.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_peers::Peers;
use aws_testkit::{Harness, TempDir};
use aws_wire::service::Caller;
use doze_runtime::{Ctx, ManualClock, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Sns;
use crate::core::Core;

mod delivery;
mod filters;
mod persistence;
mod platform;
mod publishing;
mod refusals;
mod subscriptions;
mod topics;

pub(crate) const START: i64 = 1_700_000_000_000;
pub(crate) const ACCOUNT: &str = "000000000000";

pub(crate) fn temp() -> TempDir {
    TempDir::new("sns")
}

fn ctx(dir: &std::path::Path, ext: Option<Arc<dyn std::any::Any + Send + Sync>>) -> Arc<Ctx> {
    Arc::new(Ctx {
        dir: dir.to_path_buf(),
        idle: std::time::Duration::from_secs(300),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext,
    })
}

/// A region of SNS, and SQS beside it on the bus, on a runtime the test drives.
pub(crate) struct World {
    pub(crate) core: Core,
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) peers: Arc<Peers>,
    pub(crate) rt: tokio::runtime::Runtime,
    h: Harness<Sns>,
    _dir: TempDir,
}

impl World {
    pub(crate) fn new() -> Self {
        let dir = temp();
        let peers = Peers::new();
        // As the binary starts: policies are asked, and a refusal is logged and goes through.
        peers.set_mode(aws_peers::Mode::Soft);
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        peers.register(Arc::new(aws_sqs::Sqs::new(ctx(
            &dir.path().join("sqs"),
            Some(ext),
        ))));
        let clock = Arc::new(ManualClock::at(START));
        let core = Core::in_memory(clock.clone(), Some(peers.clone()));
        Self::with(core, clock, peers, dir)
    }

    pub(crate) fn with(
        core: Core,
        clock: Arc<ManualClock>,
        peers: Arc<Peers>,
        dir: TempDir,
    ) -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        Self {
            core,
            clock,
            peers,
            rt,
            h: Harness::new(Sns::new(ctx(
                &std::env::temp_dir().join("doze-aws-sns-unused"),
                None,
            ))),
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

    /// How many rows the call changed: it is run and staged, and its rows are counted, not written.
    pub(crate) fn rows_of(&mut self, op: &str, request: &Value) -> usize {
        let ix = self.h.op(op);
        let call = self.h.call(ix, request).unwrap();
        self.rt.block_on(self.core.dispatch(&call)).unwrap();
        let c = &mut self.core;
        c.topics.stage().unwrap().rows()
            + c.subs.stage().unwrap().rows()
            + c.apps.stage().unwrap().rows()
            + c.endpoints.stage().unwrap().rows()
            + c.sms.stage().unwrap().rows()
    }

    /// A call as the actor takes it: authorized first, then run.
    pub(crate) fn handled(&mut self, op: &str, request: &Value) -> Result<(), aws_wire::ApiError> {
        use doze_runtime::Service;
        let ix = self.h.op(op);
        let call = self.h.call(ix, request)?;
        self.rt.block_on(self.core.handle(call)).reply.map(|_| ())
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

    /// The message of a refusal.
    pub(crate) fn refused(&mut self, op: &str, request: &Value) -> String {
        self.err(op, request).1
    }

    pub(crate) fn served(&self, op: &str) -> bool {
        self.h.serves(op)
    }

    /// A call to SQS, as the account.
    pub(crate) fn sqs(&self, op: &str, body: &Value) -> Result<Value, aws_wire::ApiError> {
        let caller = Caller {
            access_key_id: "test".into(),
            account_id: ACCOUNT.into(),
            region: "us-east-1".into(),
            endpoint: "http://localhost:4566".into(),
            service: None,
            session_token: None,
        };
        self.rt
            .block_on(self.peers.call("test", &caller, "sqs", op, body))
    }

    /// Makes a queue that the topic may send to; returns its ARN.
    pub(crate) fn queue(&self, name: &str, topic_arn: &str, extra: Value) -> String {
        let mut attributes = extra.as_object().cloned().unwrap_or_default();
        attributes.insert("Policy".into(), json!(allow_sns(name, topic_arn)));
        let made = self
            .sqs(
                "CreateQueue",
                &json!({"QueueName": name, "Attributes": attributes}),
            )
            .unwrap();
        let attrs = self
            .sqs(
                "GetQueueAttributes",
                &json!({"QueueUrl": made["QueueUrl"], "AttributeNames": ["QueueArn"]}),
            )
            .unwrap();
        attrs["Attributes"]["QueueArn"].as_str().unwrap().to_owned()
    }

    pub(crate) fn queue_url(name: &str) -> String {
        format!("http://localhost:4566/{ACCOUNT}/{name}")
    }

    /// Everything waiting in a queue, bodies and attributes.
    pub(crate) fn drain(&self, name: &str) -> Vec<Value> {
        let got = self
            .sqs(
                "ReceiveMessage",
                &json!({"QueueUrl": Self::queue_url(name), "MaxNumberOfMessages": 10,
                    "MessageAttributeNames": ["All"], "MessageSystemAttributeNames": ["All"]}),
            )
            .unwrap();
        got["Messages"].as_array().cloned().unwrap_or_default()
    }

    /// A topic, made; returns its ARN.
    pub(crate) fn topic(&mut self, name: &str) -> String {
        self.ok("CreateTopic", &json!({"Name": name}))["TopicArn"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Subscribes a queue to a topic; returns the subscription's ARN.
    pub(crate) fn subscribe_queue(
        &mut self,
        topic: &str,
        queue_arn: &str,
        attrs: &Value,
    ) -> String {
        self.ok(
            "Subscribe",
            &json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": queue_arn, "Attributes": attrs}),
        )["SubscriptionArn"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

/// A queue policy that lets the topic send to the queue.
pub(crate) fn allow_sns(queue: &str, topic_arn: &str) -> String {
    json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": {"Service": "sns.amazonaws.com"},
            "Action": "sqs:SendMessage",
            "Resource": format!("arn:aws:sqs:us-east-1:{ACCOUNT}:{queue}"),
            "Condition": {"ArnEquals": {"aws:SourceArn": topic_arn}}
        }]
    })
    .to_string()
}

pub(crate) fn arn(name: &str) -> String {
    format!("arn:aws:sns:us-east-1:{ACCOUNT}:{name}")
}

#[test]
fn the_sandbox_and_origination_numbers_are_refused_by_name() {
    let w = World::new();
    for op in [
        "CreateTopic",
        "Publish",
        "PublishBatch",
        "Subscribe",
        "CreatePlatformApplication",
        "SetSMSAttributes",
    ] {
        assert!(w.served(op), "{op} is built");
    }
    for op in [
        "CreateSMSSandboxPhoneNumber",
        "DeleteSMSSandboxPhoneNumber",
        "GetSMSSandboxAccountStatus",
        "ListSMSSandboxPhoneNumbers",
        "VerifySMSSandboxPhoneNumber",
        "ListOriginationNumbers",
    ] {
        assert!(!w.served(op), "{op} is refused");
    }
}
