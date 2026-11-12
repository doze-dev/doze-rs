//! The service driven through the peer bus, as another service would call it, with SQS, SNS, DynamoDB, EventBridge and CloudWatch
//! Logs running for real beside it, so a task's effect is read back from the service it reached.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

mod executions;
mod integrations;
mod lifecycle;

use std::sync::Arc;
use std::time::Duration;

use aws_peers::Peers;
use aws_testkit::TempDir;
use aws_wire::ApiError;
use aws_wire::service::Caller;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Sfn;

pub(crate) const ACCOUNT: &str = "000000000000";
pub(crate) const ROLE: &str = "arn:aws:iam::000000000000:role/states";

/// Step Functions and the services its tasks reach, on one bus.
pub(crate) struct World {
    pub(crate) sfn: Arc<Sfn>,
    pub(crate) peers: Arc<Peers>,
    pub(crate) tasks: Tasks,
    pub(crate) dir: TempDir,
}

pub(crate) fn caller() -> Caller {
    Caller {
        access_key_id: "test".into(),
        account_id: ACCOUNT.into(),
        region: "us-east-1".into(),
        endpoint: "http://127.0.0.1:4566".into(),
        service: None,
        session_token: None,
    }
}

impl World {
    pub(crate) fn new() -> Self {
        Self::at(TempDir::new("sfn"))
    }

    /// A world over `dir` (a restart reuses one).
    pub(crate) fn at(dir: TempDir) -> Self {
        let tasks = Tasks::new();
        let peers = Peers::new();
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        let ctx = |name: &str| {
            Arc::new(Ctx {
                dir: dir.path().join(name),
                idle: Duration::from_secs(300),
                tasks: tasks.clone(),
                clock: Arc::new(SystemClock),
                ext: Some(Arc::clone(&ext)),
            })
        };
        peers.register(Arc::new(aws_sqs::Sqs::new(ctx("sqs"))));
        peers.register(Arc::new(aws_sns::Sns::new(ctx("sns"))));
        peers.register(Arc::new(aws_dynamodb::Dynamodb::new(ctx("dynamodb"))));
        peers.register(Arc::new(aws_logs::Logs::new(ctx("logs"))));
        peers.register(Arc::new(aws_eventbridge::Eventbridge::new(ctx("events"))));
        let sfn = Arc::new(Sfn::new(ctx("states")));
        peers.register(Arc::clone(&sfn) as Arc<dyn aws_wire::service::Service>);
        Self {
            sfn,
            peers,
            tasks,
            dir,
        }
    }

    pub(crate) async fn call(&self, op: &str, body: Value) -> Result<Value, ApiError> {
        self.peers
            .call("test", &caller(), "states", op, &body)
            .await
    }

    pub(crate) async fn ok(&self, op: &str, body: Value) -> Value {
        match self.call(op, body).await {
            Ok(v) => v,
            Err(e) => panic!("{op} failed: {} {}", e.code, e.message),
        }
    }

    pub(crate) async fn peer(&self, service: &str, op: &str, body: Value) -> Value {
        match self.peers.call("test", &caller(), service, op, &body).await {
            Ok(v) => v,
            Err(e) => panic!("{service}.{op} failed: {} {}", e.code, e.message),
        }
    }

    /// A Standard machine (or an Express one when `express`) with `definition`.
    pub(crate) async fn machine_of(&self, name: &str, definition: &Value, express: bool) -> String {
        let made = self
            .ok(
                "CreateStateMachine",
                json!({"name": name, "definition": definition.to_string(), "roleArn": ROLE,
                    "type": if express { "EXPRESS" } else { "STANDARD" }}),
            )
            .await;
        made["stateMachineArn"].as_str().unwrap().to_owned()
    }

    pub(crate) async fn machine(&self, name: &str, definition: Value) -> String {
        self.machine_of(name, &definition, false).await
    }

    pub(crate) async fn start(&self, arn: &str, input: Value) -> String {
        let started = self
            .ok(
                "StartExecution",
                json!({"stateMachineArn": arn, "input": input.to_string()}),
            )
            .await;
        started["executionArn"].as_str().unwrap().to_owned()
    }

    /// Waits for an execution to stop and answers its description.
    pub(crate) async fn finished(&self, execution: &str) -> Value {
        for _ in 0..200 {
            let d = self
                .ok("DescribeExecution", json!({"executionArn": execution}))
                .await;
            if d["status"] != "RUNNING" {
                return d;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("{execution} is still running");
    }

    pub(crate) async fn run(&self, arn: &str, input: Value) -> Value {
        let e = self.start(arn, input).await;
        self.finished(&e).await
    }

    /// The types of an execution's history events.
    pub(crate) async fn history(&self, execution: &str) -> Vec<String> {
        let h = self
            .ok(
                "GetExecutionHistory",
                json!({"executionArn": execution, "maxResults": 1000}),
            )
            .await;
        h["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["type"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Stops every task and closes every file, so the directory can be opened again.
    pub(crate) async fn stop(self) -> TempDir {
        self.tasks.shutdown().await;
        self.dir
    }
}

/// An output, read from its JSON text.
pub(crate) fn output(d: &Value) -> Value {
    serde_json::from_str(d["output"].as_str().unwrap_or("null")).unwrap()
}
