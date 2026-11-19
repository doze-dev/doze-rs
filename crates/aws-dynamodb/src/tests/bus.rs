//! DynamoDB, S3, Kinesis (and IAM, when a test installs it) on one bus, driven as the gateway would drive DynamoDB.

use std::sync::Arc;

use aws_peers::Peers;
use aws_testkit::{Harness, TempDir};
use aws_wire::ApiError;
use aws_wire::service::{Caller, Service};
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Dynamodb;

/// DynamoDB, S3, Kinesis (and IAM, when a test installs it) on one bus.
pub(crate) struct Bus {
    pub(crate) peers: Arc<Peers>,
    pub(crate) ddb: Arc<Dynamodb>,
    h: Harness<Dynamodb>,
    pub(crate) dir: TempDir,
    /// The access key calls are made with, once IAM is in force.
    key: std::sync::Mutex<Option<String>>,
}

pub(crate) fn caller() -> Caller {
    Caller {
        access_key_id: "test".into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: None,
    }
}

impl Bus {
    pub(crate) fn new() -> Self {
        let dir = TempDir::new("ddb-bus");
        let peers = Peers::new();
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        let tasks = Tasks::new();
        let ctx = |name: &str| {
            Arc::new(Ctx {
                dir: dir.path().join(name),
                idle: std::time::Duration::from_secs(300),
                tasks: tasks.clone(),
                clock: Arc::new(SystemClock),
                ext: Some(Arc::clone(&ext)),
            })
        };
        let ddb = Arc::new(Dynamodb::new(ctx("dynamodb")));
        peers.register(ddb.clone());
        peers.register(Arc::new(aws_s3::S3::new(ctx("s3"))));
        peers.register(Arc::new(aws_kinesis::Kinesis::new(ctx("kinesis"))));
        let h = Harness::new(Dynamodb::new(ctx("unused")));
        Self {
            peers,
            ddb,
            h,
            dir,
            key: std::sync::Mutex::new(None),
        }
    }

    /// Installs IAM as the authorizer, refusing what it does not allow, and calls from then on as a user whose own policy allows
    /// all of DynamoDB (the account root is allowed everything locally, whatever a resource says).
    pub(crate) async fn enforce_iam(&self) {
        let ctx = Arc::new(Ctx {
            dir: self.dir.path().join("iam"),
            idle: std::time::Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: Some(self.peers.clone()),
        });
        let (iam, _sts) = aws_iam::services(ctx);
        let iam = Arc::new(iam);
        self.peers.register(iam.clone());
        // IAM reads the mode when it opens.
        self.peers.set_authorizer(iam);
        self.peers.set_mode(aws_peers::Mode::Enforce);
        let as_root = |op: &'static str, body: Value| {
            let peers = Arc::clone(&self.peers);
            async move {
                peers
                    .call("test", &caller(), "iam", op, &body)
                    .await
                    .unwrap_or_else(|e| panic!("iam {op}: {}", e.message))
            }
        };
        as_root("CreateUser", json!({"UserName": "dev"})).await;
        let allow = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "dynamodb:*", "Resource": "*"}]});
        as_root(
            "PutUserPolicy",
            json!({"UserName": "dev", "PolicyName": "all", "PolicyDocument": allow.to_string()}),
        )
        .await;
        let key = as_root("CreateAccessKey", json!({"UserName": "dev"})).await;
        if let Ok(mut k) = self.key.lock() {
            *k = key["AccessKey"]["AccessKeyId"].as_str().map(str::to_owned);
        }
    }

    pub(crate) async fn call(&self, op: &str, req: Value) -> Result<Value, ApiError> {
        let ix = self.h.op(op);
        let mut call = self.h.call(ix, &req)?;
        if let Some(k) = self.key.lock().ok().and_then(|k| k.clone()) {
            call.caller.access_key_id = k;
        }
        let out = self.ddb.call(call).await?;
        Ok(self.h.json(ix, &out))
    }

    pub(crate) async fn ok(&self, op: &str, req: Value) -> Value {
        self.call(op, req)
            .await
            .unwrap_or_else(|e| panic!("{op} failed: {} {}", e.code, e.message))
    }

    pub(crate) async fn table(&self, name: &str) {
        self.ok(
            "CreateTable",
            json!({"TableName": name, "BillingMode": "PAY_PER_REQUEST",
                "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
                "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}),
        )
        .await;
    }

    pub(crate) async fn s3(&self, op: &str, body: Value) -> aws_wire::rest::RestReply {
        self.peers
            .call_rest("test", &caller(), "s3", op, &body)
            .await
            .unwrap_or_else(|e| panic!("s3 {op}: {}", e.message))
    }

    pub(crate) async fn object(&self, bucket: &str, key: &str) -> Vec<u8> {
        let mut reply = self
            .s3("GetObject", json!({"Bucket": bucket, "Key": key}))
            .await;
        let mut out = Vec::new();
        if let Some(mut stream) = reply.body.take() {
            while let Some(chunk) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
                out.extend_from_slice(&chunk.unwrap());
            }
        }
        out
    }

    pub(crate) async fn keys(&self, bucket: &str) -> Vec<String> {
        let listed = self.s3("ListObjectsV2", json!({"Bucket": bucket})).await;
        listed.json["Contents"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|o| o["Key"].as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) async fn put_object(&self, bucket: &str, key: &str, body: &[u8]) {
        self.peers
            .call_rest_with_body(
                "test",
                &caller(),
                ("s3", "PutObject"),
                &json!({"Bucket": bucket, "Key": key}),
                bytes::Bytes::copy_from_slice(body),
            )
            .await
            .unwrap();
    }

    pub(crate) async fn kinesis(&self, op: &str, body: Value) -> Value {
        self.peers
            .call("test", &caller(), "kinesis", op, &body)
            .await
            .unwrap_or_else(|e| panic!("kinesis {op}: {}", e.message))
    }
}
