//! What DynamoDB does with S3 and Kinesis on the bus: exports written in AWS's layout, imports read back, item changes sent to a
//! Kinesis streaming destination.

use std::io::Read as _;
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

    async fn s3(&self, op: &str, body: Value) -> aws_wire::rest::RestReply {
        self.peers
            .call_rest("test", &caller(), "s3", op, &body)
            .await
            .unwrap_or_else(|e| panic!("s3 {op}: {}", e.message))
    }

    async fn object(&self, bucket: &str, key: &str) -> Vec<u8> {
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

    async fn keys(&self, bucket: &str) -> Vec<String> {
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

    async fn put_object(&self, bucket: &str, key: &str, body: &[u8]) {
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

    async fn kinesis(&self, op: &str, body: Value) -> Value {
        self.peers
            .call("test", &caller(), "kinesis", op, &body)
            .await
            .unwrap_or_else(|e| panic!("kinesis {op}: {}", e.message))
    }
}

fn gunzip(bytes: &[u8]) -> String {
    let mut out = String::new();
    flate2::read::MultiGzDecoder::new(bytes)
        .read_to_string(&mut out)
        .unwrap();
    out
}

async fn with_recovery(bus: &Bus, name: &str) {
    bus.table(name).await;
    bus.ok(
        "UpdateContinuousBackups",
        json!({"TableName": name, "PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": true}}),
    )
    .await;
}

#[tokio::test]
async fn an_export_is_written_in_the_layout_aws_writes_and_imports_back() {
    let bus = Bus::new();
    with_recovery(&bus, "tab").await;
    for (pk, n) in [("a", "1"), ("b", "2")] {
        bus.ok("PutItem", json!({"TableName": "tab", "Item": {"pk": {"S": pk}, "n": {"N": n}, "s": {"SS": ["x"]}}}))
            .await;
    }
    bus.s3("CreateBucket", json!({"Bucket": "exports"})).await;
    let arn = "arn:aws:dynamodb:us-east-1:000000000000:table/tab";
    let out = bus
        .ok(
            "ExportTableToPointInTime",
            json!({"TableArn": arn, "S3Bucket": "exports", "S3Prefix": "nightly/"}),
        )
        .await;
    assert_eq!(out["ExportDescription"]["ExportStatus"], "IN_PROGRESS");
    let export_arn = out["ExportDescription"]["ExportArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let id = export_arn.rsplit('/').next().unwrap().to_owned();
    let done = bus
        .ok("DescribeExport", json!({"ExportArn": export_arn}))
        .await;
    let d = &done["ExportDescription"];
    assert_eq!(d["ExportStatus"], "COMPLETED", "{d}");
    assert_eq!(d["ItemCount"], 2);
    assert_eq!(d["ExportFormat"], "DYNAMODB_JSON");
    let dir = format!("nightly/AWSDynamoDB/{id}");
    assert_eq!(d["ExportManifest"], format!("{dir}/manifest-summary.json"));
    check_layout(&bus, &dir, &export_arn).await;
    let listed = bus.ok("ListExports", json!({"TableArn": arn})).await;
    assert_eq!(listed["ExportSummaries"][0]["ExportStatus"], "COMPLETED");
    import_back(&bus, &dir).await;
}

/// The files of a finished export, and what the manifests and the data say.
async fn check_layout(bus: &Bus, dir: &str, export_arn: &str) {
    let mut keys = bus.keys("exports").await;
    keys.sort();
    let data_key = keys.iter().find(|k| k.contains("/data/")).unwrap().clone();
    assert!(data_key.starts_with(&format!("{dir}/data/")) && data_key.ends_with(".json.gz"));
    for name in [
        "_started",
        "manifest-files.json",
        "manifest-files.md5",
        "manifest-summary.json",
        "manifest-summary.md5",
    ] {
        assert!(
            keys.contains(&format!("{dir}/{name}")),
            "{name} in {keys:?}"
        );
    }
    let summary: Value = serde_json::from_slice(
        &bus.object("exports", &format!("{dir}/manifest-summary.json"))
            .await,
    )
    .unwrap();
    assert_eq!(summary["exportArn"], export_arn);
    assert_eq!(summary["itemCount"], 2);
    assert_eq!(summary["outputFormat"], "DYNAMODB_JSON");
    assert_eq!(
        summary["manifestFilesS3Key"],
        format!("{dir}/manifest-files.json")
    );
    let files: Value = serde_json::from_slice(
        &bus.object("exports", &format!("{dir}/manifest-files.json"))
            .await,
    )
    .unwrap();
    assert_eq!(files["dataFileS3Key"], data_key.as_str());
    assert_eq!(files["itemCount"], 2);
    let lines = gunzip(&bus.object("exports", &data_key).await);
    let mut items: Vec<Value> = lines
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    items.sort_by_key(|i| i["Item"]["pk"]["S"].as_str().unwrap().to_owned());
    assert_eq!(
        items[0],
        json!({"Item": {"pk": {"S": "a"}, "n": {"N": "1"}, "s": {"SS": ["x"]}}})
    );
}

/// The export's data folder imports into a new table.
async fn import_back(bus: &Bus, dir: &str) {
    let imported = bus
        .ok(
            "ImportTable",
            json!({"S3BucketSource": {"S3Bucket": "exports", "S3KeyPrefix": format!("{dir}/data/")},
                "InputFormat": "DYNAMODB_JSON", "InputCompressionType": "GZIP",
                "TableCreationParameters": {"TableName": "copy", "BillingMode": "PAY_PER_REQUEST",
                    "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
                    "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}}),
        )
        .await;
    let import_arn = imported["ImportTableDescription"]["ImportArn"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        imported["ImportTableDescription"]["ImportStatus"],
        "IN_PROGRESS"
    );
    let d = bus
        .ok("DescribeImport", json!({"ImportArn": import_arn}))
        .await;
    assert_eq!(
        d["ImportTableDescription"]["ImportStatus"], "COMPLETED",
        "{d}"
    );
    assert_eq!(d["ImportTableDescription"]["ImportedItemCount"], 2);
    let got = bus
        .ok(
            "GetItem",
            json!({"TableName": "copy", "Key": {"pk": {"S": "b"}}}),
        )
        .await;
    assert_eq!(got["Item"]["n"], json!({"N": "2"}));
    let t = bus.ok("DescribeTable", json!({"TableName": "copy"})).await;
    assert_eq!(t["Table"]["TableStatus"], "ACTIVE");
    assert_eq!(
        bus.ok("ListImports", json!({})).await["ImportSummaryList"][0]["ImportStatus"],
        "COMPLETED"
    );
}

#[tokio::test]
async fn ion_exports_and_csv_imports() {
    let bus = Bus::new();
    with_recovery(&bus, "tab").await;
    bus.ok(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "n": {"N": "600"}}}),
    )
    .await;
    bus.s3("CreateBucket", json!({"Bucket": "bkt"})).await;
    let out = bus
        .ok("ExportTableToPointInTime", json!({"TableArn": "arn:aws:dynamodb:us-east-1:000000000000:table/tab", "S3Bucket": "bkt", "ExportFormat": "ION"}))
        .await;
    let id = out["ExportDescription"]["ExportArn"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    let keys = bus.keys("bkt").await;
    let data = keys.iter().find(|k| k.ends_with(".ion.gz")).unwrap();
    assert!(data.starts_with(&format!("AWSDynamoDB/{id}/data/")));
    assert_eq!(
        gunzip(&bus.object("bkt", data).await),
        "$ion_1_0 {Item:{n:600.,pk:\"a\"}}\n"
    );

    bus.put_object(
        "bkt",
        "csv/one.csv",
        b"pk,n,note\nx,1,hello\ny,2,\n,3,no key\n",
    )
    .await;
    let imported = bus
        .ok(
            "ImportTable",
            json!({"S3BucketSource": {"S3Bucket": "bkt", "S3KeyPrefix": "csv/"}, "InputFormat": "CSV",
                "TableCreationParameters": {"TableName": "fromcsv", "BillingMode": "PAY_PER_REQUEST",
                    "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
                    "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}}),
        )
        .await;
    let arn = imported["ImportTableDescription"]["ImportArn"].clone();
    let d =
        bus.ok("DescribeImport", json!({"ImportArn": arn})).await["ImportTableDescription"].clone();
    assert_eq!(
        (d["ImportedItemCount"].as_u64(), d["ErrorCount"].as_u64()),
        (Some(2), Some(1)),
        "{d}"
    );
    let y = bus
        .ok(
            "GetItem",
            json!({"TableName": "fromcsv", "Key": {"pk": {"S": "y"}}}),
        )
        .await;
    assert_eq!(y["Item"], json!({"pk": {"S": "y"}, "n": {"S": "2"}}));
}

#[tokio::test]
async fn an_export_or_import_without_a_bucket_fails_and_says_why() {
    let bus = Bus::new();
    with_recovery(&bus, "tab").await;
    let out = bus
        .ok("ExportTableToPointInTime", json!({"TableArn": "arn:aws:dynamodb:us-east-1:000000000000:table/tab", "S3Bucket": "nope"}))
        .await;
    let d = bus
        .ok(
            "DescribeExport",
            json!({"ExportArn": out["ExportDescription"]["ExportArn"]}),
        )
        .await;
    assert_eq!(d["ExportDescription"]["ExportStatus"], "FAILED");
    assert_eq!(d["ExportDescription"]["FailureCode"], "S3NoSuchBucket");
    let imported = bus
        .ok(
            "ImportTable",
            json!({"S3BucketSource": {"S3Bucket": "nope"}, "InputFormat": "DYNAMODB_JSON",
                "TableCreationParameters": {"TableName": "never", "BillingMode": "PAY_PER_REQUEST",
                    "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
                    "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}}),
        )
        .await;
    let d = bus
        .ok(
            "DescribeImport",
            json!({"ImportArn": imported["ImportTableDescription"]["ImportArn"]}),
        )
        .await;
    assert_eq!(d["ImportTableDescription"]["ImportStatus"], "FAILED");
    let e = bus
        .call("DescribeTable", json!({"TableName": "never"}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "ResourceNotFoundException");
    // Exports need recovery on.
    bus.table("plain").await;
    let e = bus
        .call("ExportTableToPointInTime", json!({"TableArn": "arn:aws:dynamodb:us-east-1:000000000000:table/plain", "S3Bucket": "bkt"}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "PointInTimeRecoveryUnavailableException");
}

#[tokio::test]
async fn item_changes_reach_a_kinesis_streaming_destination() {
    let bus = Bus::new();
    bus.table("tab").await;
    bus.kinesis(
        "CreateStream",
        json!({"StreamName": "changes", "ShardCount": 1}),
    )
    .await;
    let stream = "arn:aws:kinesis:us-east-1:000000000000:stream/changes";
    let on = bus
        .ok(
            "EnableKinesisStreamingDestination",
            json!({"TableName": "tab", "StreamArn": stream}),
        )
        .await;
    assert_eq!(on["DestinationStatus"], "ENABLING");
    let d = bus
        .ok(
            "DescribeKinesisStreamingDestination",
            json!({"TableName": "tab"}),
        )
        .await;
    assert_eq!(
        d["KinesisDataStreamDestinations"][0]["DestinationStatus"], "ACTIVE",
        "{d}"
    );
    bus.ok(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "v": {"N": "1"}}}),
    )
    .await;
    bus.ok(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "v": {"N": "2"}}}),
    )
    .await;
    bus.ok(
        "DeleteItem",
        json!({"TableName": "tab", "Key": {"pk": {"S": "a"}}}),
    )
    .await;
    let shards = bus
        .kinesis("ListShards", json!({"StreamName": "changes"}))
        .await;
    let shard = shards["Shards"][0]["ShardId"].as_str().unwrap().to_owned();
    let it = bus
        .kinesis(
            "GetShardIterator",
            json!({"StreamName": "changes", "ShardId": shard, "ShardIteratorType": "TRIM_HORIZON"}),
        )
        .await;
    let got = bus
        .kinesis("GetRecords", json!({"ShardIterator": it["ShardIterator"]}))
        .await;
    check_records(&got);
    then_disabled(&bus, stream, &got).await;
}

/// The three records of an insert, a change and a delete, in the shape of the guide.
fn check_records(got: &Value) {
    let records: Vec<Value> = got["Records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            serde_json::from_slice(&aws_core::base64::decode(r["Data"].as_str().unwrap()).unwrap())
                .unwrap()
        })
        .collect();
    let events: Vec<&str> = records
        .iter()
        .map(|r| r["eventName"].as_str().unwrap())
        .collect();
    assert_eq!(events, ["INSERT", "MODIFY", "REMOVE"]);
    let modify = &records[1];
    assert_eq!(modify["tableName"], "tab");
    assert_eq!(modify["eventSource"], "aws:dynamodb");
    assert_eq!(modify["recordFormat"], "application/json");
    assert_eq!(modify["dynamodb"]["Keys"], json!({"pk": {"S": "a"}}));
    assert_eq!(modify["dynamodb"]["OldImage"]["v"], json!({"N": "1"}));
    assert_eq!(modify["dynamodb"]["NewImage"]["v"], json!({"N": "2"}));
    assert_eq!(
        modify["dynamodb"]["ApproximateCreationDateTimePrecision"],
        "MILLISECOND"
    );
}

/// Microseconds when asked; nothing once disabled; a stream that is not there fails to enable.
async fn then_disabled(bus: &Bus, stream: &str, got: &Value) {
    let up = bus
        .ok(
            "UpdateKinesisStreamingDestination",
            json!({"TableName": "tab", "StreamArn": stream, "UpdateKinesisStreamingConfiguration": {"ApproximateCreationDateTimePrecision": "MICROSECOND"}}),
        )
        .await;
    assert_eq!(up["DestinationStatus"], "UPDATING");
    let off = bus
        .ok(
            "DisableKinesisStreamingDestination",
            json!({"TableName": "tab", "StreamArn": stream}),
        )
        .await;
    assert_eq!(off["DestinationStatus"], "DISABLING");
    bus.ok(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "b"}}}),
    )
    .await;
    let later = bus
        .kinesis(
            "GetRecords",
            json!({"ShardIterator": got["NextShardIterator"]}),
        )
        .await;
    assert_eq!(later["Records"], json!([]));
    let d = bus
        .ok(
            "DescribeKinesisStreamingDestination",
            json!({"TableName": "tab"}),
        )
        .await;
    assert_eq!(
        d["KinesisDataStreamDestinations"][0]["DestinationStatus"],
        "DISABLED"
    );
    // A stream that is not there fails to enable, and says why.
    let missing = "arn:aws:kinesis:us-east-1:000000000000:stream/none";
    bus.ok(
        "EnableKinesisStreamingDestination",
        json!({"TableName": "tab", "StreamArn": missing}),
    )
    .await;
    let d = bus
        .ok(
            "DescribeKinesisStreamingDestination",
            json!({"TableName": "tab"}),
        )
        .await;
    let failed = d["KinesisDataStreamDestinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["StreamArn"] == missing)
        .unwrap()
        .clone();
    assert_eq!(failed["DestinationStatus"], "ENABLE_FAILED");
    assert!(failed.get("DestinationStatusDescription").is_some());
}
