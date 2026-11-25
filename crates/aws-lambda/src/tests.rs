//! Lambda end to end inside the crate: real processes behind the real Runtime API, driven through the service the way a gateway
//! would. A runtime that the host lacks makes its test skip (it says so), not fail.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used
)]

use std::sync::Arc;
use std::time::Duration;

use aws_core::base64;
use aws_peers::Peers;
use aws_testkit::{Harness, TempDir};
use aws_wire::ApiError;
use aws_wire::rest::RestReply;
use aws_wire::service::Caller;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::{Config, Lambda};

mod behaviour;
mod concurrency;
mod data;
mod durable;
mod durable_js;
mod forced;
mod functions;
mod invoking;
mod livelog;
mod reshard;
mod sources;
mod url_auth;

pub(crate) const ACCOUNT: &str = "111122223333";

/// A Lambda on a temporary directory, with its tasks, driven by JSON requests.
pub(crate) struct World {
    pub(crate) lambda: Lambda,
    h: Harness<Lambda>,
    pub(crate) tasks: Tasks,
    pub(crate) peers: Arc<Peers>,
    /// Where functions reach the services (a real gateway once [`Self::serve`] started one).
    pub(crate) endpoint: String,
    gateway: Option<aws_gateway::Running>,
    _dir: TempDir,
}

impl World {
    pub(crate) fn new() -> Self {
        Self::with_mode(aws_peers::Mode::Off)
    }

    /// Lambda with SQS, Kinesis, DynamoDB, CloudWatch Logs, IAM and STS beside it on the bus, as the binary has them, and the policies
    /// of `mode` asked.
    pub(crate) fn with_mode(mode: aws_peers::Mode) -> Self {
        Self::build(
            mode,
            Config {
                quiet: true,
                ..Config::default()
            },
        )
    }

    /// Lambda with its settings, as [`Self::with_mode`] has it otherwise.
    pub(crate) fn with_config(config: Config) -> Self {
        Self::build(aws_peers::Mode::Off, config)
    }

    fn build(mode: aws_peers::Mode, config: Config) -> Self {
        let dir = TempDir::new("lambda");
        let tasks = Tasks::new();
        let peers = Peers::new();
        peers.set_mode(mode);
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        let ctx_in = |name: &str| {
            Arc::new(Ctx {
                dir: dir.path().join(name),
                idle: Duration::from_secs(300),
                tasks: tasks.clone(),
                clock: Arc::new(SystemClock),
                ext: Some(Arc::clone(&ext)),
            })
        };
        let ctx = ctx_in("lambda");
        let sqs = Arc::new(aws_sqs::Sqs::new(ctx_in("sqs")));
        let kinesis = Arc::new(aws_kinesis::Kinesis::new(ctx_in("kinesis")));
        let dynamodb = Arc::new(aws_dynamodb::Dynamodb::new(ctx_in("dynamodb")));
        let (iam, sts) = aws_iam::services(ctx_in("iam"));
        let iam = Arc::new(iam);
        peers.set_authorizer(iam.clone());
        peers.register(sqs.clone());
        peers.register(kinesis.clone());
        peers.register(dynamodb.clone());
        peers.register(Arc::new(aws_logs::Logs::new(ctx_in("logs"))));
        peers.register(iam);
        peers.register(Arc::new(sts));
        let lambda = Lambda::with_config(ctx, config);
        lambda.connect(crate::Sources {
            sqs,
            kinesis,
            dynamodb,
        });
        peers.register(Arc::new(lambda.clone()));
        let h = Harness::new(lambda.clone());
        Self {
            lambda,
            h,
            tasks,
            peers,
            endpoint: "http://127.0.0.1:4566".to_owned(),
            gateway: None,
            _dir: dir,
        }
    }

    /// A call to another service on the bus, as the account's root.
    pub(crate) async fn peer(&self, service: &str, op: &str, body: Value) -> Value {
        match self
            .peers
            .call("test", &Self::caller(), service, op, &body)
            .await
        {
            Ok(v) => v,
            Err(e) => panic!("{service}.{op} failed: {} {}", e.code, e.message),
        }
    }

    /// Waits until `check` holds, up to twenty seconds.
    pub(crate) async fn eventually<F, Fut>(&self, what: &str, mut check: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        for _ in 0..200 {
            if check().await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("never happened: {what}");
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

    /// One operation, as the gateway would call it, with the request as the JSON it builds from path, query, headers and body.
    pub(crate) async fn call(&self, op: &str, request: Value) -> Result<RestReply, ApiError> {
        let call = self
            .h
            .call(self.h.op(op), &request)
            .map_err(|e| ApiError::new(400, "ValidationException", e.message))?;
        let call = aws_wire::service::Call {
            caller: Caller {
                endpoint: self.endpoint.clone(),
                ..Self::caller()
            },
            ..call
        };
        let rest = aws_wire::service::Service::rest(&self.lambda).expect("lambda has a REST side");
        rest.call_rest(call, None).await
    }

    pub(crate) async fn ok(&self, op: &str, request: Value) -> Value {
        match self.call(op, request).await {
            Ok(r) => r.json,
            Err(e) => panic!("{op} failed: {} {}", e.code, e.message),
        }
    }

    pub(crate) async fn err(&self, op: &str, request: Value) -> ApiError {
        self.call(op, request)
            .await
            .expect_err("the call should have failed")
    }

    /// Creates a function whose code is `files`.
    pub(crate) async fn create(
        &self,
        name: &str,
        runtime: &str,
        handler: &str,
        files: &[(&str, &[u8], u32)],
    ) -> Value {
        let zip = crate::zipread::build(files);
        self.ok(
            "CreateFunction",
            json!({"FunctionName": name, "Runtime": runtime, "Role": format!("arn:aws:iam::{ACCOUNT}:role/r"), "Handler": handler,
                "Code": {"ZipFile": base64::encode(&zip)}, "Timeout": 10}),
        )
        .await
    }

    /// Invokes and returns the reply with its payload decoded.
    pub(crate) async fn invoke(&self, name: &str, payload: &Value) -> Value {
        let mut reply = self
            .ok("Invoke", json!({"FunctionName": name, "Payload": base64::encode(payload.to_string().as_bytes())}))
            .await;
        let bytes =
            base64::decode(reply["Payload"].as_str().unwrap_or_default()).unwrap_or_default();
        reply["Payload"] = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        reply
    }

    /// The directory the services' files are in (Lambda's is `lambda`).
    pub(crate) fn dir(&self) -> &std::path::Path {
        self._dir.path()
    }

    pub(crate) fn lambda_op(&self, name: &str) -> u32 {
        self.h.op(name)
    }

    /// Starts a real gateway in front of Lambda, for functions whose SDK calls back (durable functions).
    pub(crate) async fn serve(&mut self) {
        let gateway = aws_gateway::Gateway::new(aws_gateway::Config::new(
            "127.0.0.1:0".parse().expect("an address"),
        ))
        .with(Arc::new(self.lambda.clone()));
        let running = gateway.bind().await.expect("the gateway binds");
        self.endpoint = format!("http://{}", running.local_addr());
        self.gateway = Some(running);
    }

    pub(crate) async fn stop(self) {
        if let Some(g) = self.gateway {
            g.shutdown().await;
        }
        self.tasks.shutdown().await;
    }
}

/// Whether a program is on `PATH`; a test that needs one it lacks says so and passes.
pub(crate) fn have(program: &str) -> bool {
    let found = std::process::Command::new(program)
        .arg("--version")
        .output()
        .is_ok();
    if !found {
        eprintln!("skipped: {program} is not installed");
    }
    found
}
