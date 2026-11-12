//! The service as the gateway holds it: regions kept apart, and a stream that tells its consumers when it appends.

use std::sync::Arc;

use aws_testkit::{Harness, TempDir};
use aws_wire::service::Service;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Dynamodb;

fn service(dir: &TempDir) -> Dynamodb {
    Dynamodb::new(Arc::new(Ctx {
        dir: dir.path().to_path_buf(),
        idle: std::time::Duration::from_secs(300),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: None,
    }))
}

struct Client {
    svc: Dynamodb,
    h: Harness<Dynamodb>,
}

impl Client {
    async fn call(&self, region: &str, op: &str, body: Value) -> Result<Value, aws_wire::ApiError> {
        let ix = self.h.op(op);
        let mut call = self.h.call(ix, &body)?;
        call.caller.region = region.to_owned();
        let out = self.svc.call(call).await?;
        Ok(self.h.json(ix, &out))
    }
}

fn client(dir: &TempDir) -> Client {
    Client {
        svc: service(dir),
        h: Harness::new(service(&TempDir::new("ddb-decode"))),
    }
}

fn create(name: &str) -> Value {
    json!({"TableName": name, "BillingMode": "PAY_PER_REQUEST",
        "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
        "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}],
        "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES"}})
}

#[tokio::test(flavor = "current_thread")]
async fn a_table_belongs_to_the_region_that_made_it() {
    let dir = TempDir::new("ddb-regions");
    let c = client(&dir);
    c.call("eu-west-1", "CreateTable", create("orders"))
        .await
        .unwrap();
    let eu = c.call("eu-west-1", "ListTables", json!({})).await.unwrap();
    let us = c.call("us-east-1", "ListTables", json!({})).await.unwrap();
    assert_eq!(
        (eu["TableNames"].clone(), us["TableNames"].clone()),
        (json!(["orders"]), json!([]))
    );
    let e = c
        .call("us-east-1", "DescribeTable", json!({"TableName": "orders"}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "ResourceNotFoundException");
    // The same name is free in the other region.
    c.call("us-east-1", "CreateTable", create("orders"))
        .await
        .unwrap();
    // The home region keeps its files in the service's directory itself; another region has one of its own.
    assert!(dir.path().join("dynamodb.redb").exists());
    assert!(
        dir.path()
            .join("regions")
            .join("eu-west-1")
            .join("dynamodb.redb")
            .exists()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_stream_wakes_the_consumer_waiting_for_it() {
    let dir = TempDir::new("ddb-stream");
    let c = client(&dir);
    let made = c
        .call("us-east-1", "CreateTable", create("orders"))
        .await
        .unwrap();
    let arn = made["TableDescription"]["LatestStreamArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let empty = c
        .svc
        .stream_records("us-east-1", &arn, 0, 10)
        .await
        .unwrap();
    assert!(empty.records.is_empty() && empty.open);

    let notified = c.svc.arrived.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    c.call(
        "us-east-1",
        "PutItem",
        json!({"TableName": "orders", "Item": {"pk": {"S": "a"}}}),
    )
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), notified)
        .await
        .unwrap();

    let batch = c
        .svc
        .stream_records("us-east-1", &arn, 0, 10)
        .await
        .unwrap();
    assert_eq!(batch.records.len(), 1);
    assert_eq!(
        batch.records[0].to_event("us-east-1")["eventName"],
        "INSERT"
    );
    let none = c.svc.stream_records("eu-west-1", &arn, 0, 10).await;
    assert!(none.is_err(), "another region has no such stream");
    // A read changes nothing and wakes nobody.
    let quiet = c.svc.arrived.notified();
    tokio::pin!(quiet);
    quiet.as_mut().enable();
    c.call(
        "us-east-1",
        "GetItem",
        json!({"TableName": "orders", "Key": {"pk": {"S": "a"}}}),
    )
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), quiet)
            .await
            .is_err()
    );
}
