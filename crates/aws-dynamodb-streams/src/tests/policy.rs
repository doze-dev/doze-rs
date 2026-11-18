//! A stream's resource policy is weighed by IAM with a Streams API request, as the table's is with a DynamoDB one.

use std::sync::Arc;
use std::time::Duration;

use aws_testkit::{Harness, TempDir};
use aws_wire::service::Service;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use super::super::*;

fn caller(key: &str) -> Caller {
    Caller {
        access_key_id: key.into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: None,
    }
}

struct Iam {
    ddb: Arc<Dynamodb>,
    streams: Streams,
    ddb_h: Harness<Dynamodb>,
    streams_h: Harness<Streams>,
    key: String,
    _dir: TempDir,
}

impl Iam {
    /// DynamoDB and IAM on one bus, IAM in force, and a user whose own policy allows all of DynamoDB.
    async fn new() -> Self {
        let dir = TempDir::new("ddb-streams-iam");
        let peers = Peers::new();
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        let ctx = |name: &str| {
            Arc::new(Ctx {
                dir: dir.path().join(name),
                idle: Duration::from_secs(300),
                tasks: Tasks::new(),
                clock: Arc::new(SystemClock),
                ext: Some(Arc::clone(&ext)),
            })
        };
        let ddb = Arc::new(Dynamodb::new(ctx("dynamodb")));
        peers.register(ddb.clone());
        let (iam, _sts) = aws_iam::services(ctx("iam"));
        let iam = Arc::new(iam);
        peers.register(iam.clone());
        peers.set_authorizer(iam);
        peers.set_mode(aws_peers::Mode::Enforce);
        let root = caller("test");
        for (op, body) in [
            ("CreateUser", json!({"UserName": "dev"})),
            (
                "PutUserPolicy",
                json!({"UserName": "dev", "PolicyName": "all", "PolicyDocument": json!({"Version": "2012-10-17",
                    "Statement": [{"Effect": "Allow", "Action": "dynamodb:*", "Resource": "*"}]}).to_string()}),
            ),
        ] {
            peers.call("test", &root, "iam", op, &body).await.unwrap();
        }
        let made = peers
            .call(
                "test",
                &root,
                "iam",
                "CreateAccessKey",
                &json!({"UserName": "dev"}),
            )
            .await
            .unwrap();
        let key = made["AccessKey"]["AccessKeyId"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            streams: Streams::new(Arc::clone(&ddb), Some(peers)),
            streams_h: Harness::new(Streams::new(Arc::new(Dynamodb::new(ctx("decode"))), None)),
            ddb_h: Harness::new(Dynamodb::new(ctx("decode-ddb"))),
            ddb,
            key,
            _dir: dir,
        }
    }

    async fn ddb(&self, op: &str, body: Value) -> Value {
        let ix = self.ddb_h.op(op);
        let mut call = self.ddb_h.call(ix, &body).unwrap();
        call.caller.access_key_id.clone_from(&self.key);
        let out = self
            .ddb
            .call(call)
            .await
            .unwrap_or_else(|e| panic!("{op}: {} {}", e.code, e.message));
        self.ddb_h.json(ix, &out)
    }

    async fn streams(&self, op: &str, body: Value) -> Result<Value, ApiError> {
        let ix = self.streams_h.op(op);
        let mut call = self.streams_h.call(ix, &body)?;
        call.caller.access_key_id.clone_from(&self.key);
        let out = self.streams.call(call).await?;
        Ok(self.streams_h.json(ix, &out))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_stream_policy_denies_what_the_callers_own_policy_allows() {
    let w = Iam::new().await;
    let made = w
        .ddb(
            "CreateTable",
            json!({"TableName": "orders", "BillingMode": "PAY_PER_REQUEST",
                "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
                "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}],
                "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_IMAGE"}}),
        )
        .await;
    let arn = made["TableDescription"]["LatestStreamArn"]
        .as_str()
        .unwrap()
        .to_owned();
    w.streams("DescribeStream", json!({"StreamArn": arn}))
        .await
        .unwrap();
    let deny = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Deny", "Principal": "*",
        "Action": "dynamodb:DescribeStream", "Resource": arn}]});
    w.ddb(
        "PutResourcePolicy",
        json!({"ResourceArn": arn, "Policy": deny.to_string()}),
    )
    .await;
    let e = w
        .streams("DescribeStream", json!({"StreamArn": arn}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "AccessDeniedException", "{}", e.message);
    assert!(
        e.message.contains("dynamodb:DescribeStream"),
        "{}",
        e.message
    );
    // What the policy does not name is still allowed.
    let shard = w
        .streams(
            "GetShardIterator",
            json!({"StreamArn": arn, "ShardId": "x", "ShardIteratorType": "LATEST"}),
        )
        .await;
    assert!(
        shard
            .as_ref()
            .err()
            .is_none_or(|e| e.code != "AccessDeniedException"),
        "{shard:?}"
    );
    w.ddb("DeleteResourcePolicy", json!({"ResourceArn": arn}))
        .await;
    w.streams("DescribeStream", json!({"StreamArn": arn}))
        .await
        .unwrap();
}
