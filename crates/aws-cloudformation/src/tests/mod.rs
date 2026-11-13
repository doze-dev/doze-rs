//! CloudFormation with the real services beside it on a peer bus: a template in, the other services' state out.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]

mod cdk;
mod changes;
mod stacks;
mod templates;

use std::sync::Arc;
use std::time::Duration;

use aws_peers::Peers;
use aws_testkit::TempDir;
use aws_wire::ApiError;
use aws_wire::service::{Caller, Service};
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::{Cloudformation, Config};

/// A bus with CloudFormation and the services it makes resources in.
pub(super) struct World {
    pub(super) peers: Arc<Peers>,
    pub(super) caller: Caller,
    _dir: TempDir,
}

impl World {
    pub(super) fn new() -> Self {
        Self::with(Config::default())
    }

    pub(super) fn with(config: Config) -> Self {
        let dir = TempDir::new("cfn");
        let peers = Peers::new();
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        let tasks = Tasks::new();
        let ctx = |name: &str| {
            Arc::new(Ctx {
                dir: dir.path().join(name),
                idle: Duration::from_secs(60),
                tasks: tasks.clone(),
                clock: Arc::new(SystemClock),
                ext: Some(Arc::clone(&ext)),
            })
        };
        let (iam, sts) = aws_iam::services(ctx("iam"));
        let services: Vec<Arc<dyn Service>> = vec![
            Arc::new(iam),
            Arc::new(sts),
            Arc::new(aws_sqs::Sqs::new(ctx("sqs"))),
            Arc::new(aws_sns::Sns::new(ctx("sns"))),
            Arc::new(aws_ssm::Ssm::new(ctx("ssm"))),
            Arc::new(aws_kms::Kms::new(ctx("kms"))),
            Arc::new(aws_secrets::Secrets::new(ctx("secretsmanager"))),
            Arc::new(aws_s3::S3::new(ctx("s3"))),
            Arc::new(aws_dynamodb::Dynamodb::new(ctx("dynamodb"))),
            Arc::new(Cloudformation::with_config(ctx("cloudformation"), config)),
        ];
        for s in services {
            peers.register(s);
        }
        Self {
            peers,
            caller: Caller {
                access_key_id: "test".into(),
                account_id: "111122223333".into(),
                region: "us-east-1".into(),
                endpoint: "http://localhost:4566".into(),
                service: None,
                session_token: None,
            },
            _dir: dir,
        }
    }

    /// Calls `op` of `service`.
    pub(super) async fn call(
        &self,
        service: &str,
        op: &str,
        body: Value,
    ) -> Result<Value, ApiError> {
        self.peers
            .call("test", &self.caller, service, op, &body)
            .await
    }

    pub(super) async fn cfn(&self, op: &str, body: Value) -> Result<Value, ApiError> {
        self.call("cloudformation", op, body).await
    }

    /// Creates a stack from `template` (a JSON value or YAML text) and waits for it to settle; its status.
    pub(super) async fn create(
        &self,
        name: &str,
        template: Value,
        params: &[(&str, &str)],
    ) -> String {
        let body = match template {
            Value::String(s) => s,
            other => other.to_string(),
        };
        let params: Vec<Value> = params
            .iter()
            .map(|(k, v)| json!({"ParameterKey": k, "ParameterValue": v}))
            .collect();
        self.cfn(
            "CreateStack",
            json!({"StackName": name, "TemplateBody": body, "Parameters": params,
                "Capabilities": ["CAPABILITY_NAMED_IAM", "CAPABILITY_AUTO_EXPAND"]}),
        )
        .await
        .unwrap();
        self.settle(name).await
    }

    /// Waits until the stack is no longer in progress; its status.
    pub(super) async fn settle(&self, name: &str) -> String {
        for _ in 0..400 {
            let got = self.cfn("DescribeStacks", json!({"StackName": name})).await;
            let status = match got {
                Ok(v) => v["Stacks"][0]["StackStatus"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                Err(e) => return e.message,
            };
            if !status.ends_with("_IN_PROGRESS") {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("stack {name} did not settle");
    }

    /// The stack's outputs as a map.
    pub(super) async fn outputs(&self, name: &str) -> serde_json::Map<String, Value> {
        let got = self
            .cfn("DescribeStacks", json!({"StackName": name}))
            .await
            .unwrap();
        got["Stacks"][0]["Outputs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| {
                (
                    o["OutputKey"].as_str().unwrap().to_owned(),
                    o["OutputValue"].clone(),
                )
            })
            .collect()
    }

    /// The stack's events, oldest first, as (logical id, status, reason).
    pub(super) async fn events(&self, name: &str) -> Vec<(String, String, String)> {
        let got = self
            .cfn("DescribeStackEvents", json!({"StackName": name}))
            .await
            .unwrap();
        let mut out: Vec<(String, String, String)> = got["StackEvents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["LogicalResourceId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    e["ResourceStatus"].as_str().unwrap_or_default().to_owned(),
                    e["ResourceStatusReason"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                )
            })
            .collect();
        out.reverse();
        out
    }

    /// The status reason of a resource's event with `status`.
    pub(super) async fn reason(&self, name: &str, logical: &str, status: &str) -> String {
        self.events(name)
            .await
            .into_iter()
            .find(|(l, s, _)| l == logical && s == status)
            .map(|(_, _, r)| r)
            .unwrap_or_default()
    }

    pub(super) async fn delete(&self, name: &str) -> String {
        self.cfn("DeleteStack", json!({"StackName": name}))
            .await
            .unwrap();
        let id = self.stack_id(name).await;
        match id {
            Some(id) => self.settle(&id).await,
            None => "DELETE_COMPLETE".into(),
        }
    }

    async fn stack_id(&self, name: &str) -> Option<String> {
        let got = self.cfn("ListStacks", json!({})).await.unwrap();
        got["StackSummaries"]
            .as_array()?
            .iter()
            .find(|s| s["StackName"] == name)
            .and_then(|s| s["StackId"].as_str().map(str::to_owned))
    }
}
