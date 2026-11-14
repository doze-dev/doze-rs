//! What is refused by name, and the authorization requests.

use aws_model_dynamodb::Operation;
use aws_testkit::Harness;
use aws_wire::service::{Caller, Service};
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::json;
use std::sync::Arc;

use crate::Dynamodb;
use crate::core::built;

fn service() -> Dynamodb {
    Dynamodb::new(Arc::new(Ctx {
        dir: std::env::temp_dir().join("doze-ddb-refusals"),
        idle: std::time::Duration::from_secs(60),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: None,
    }))
}

#[test]
fn what_is_not_built_is_refused_by_name_before_it_is_read() {
    let svc = service();
    for op in Operation::ALL {
        assert_eq!(svc.serves(op.index()), built(*op), "{op:?}");
    }
    let refused = [
        "CreateGlobalTable",
        "UpdateGlobalTable",
        "DescribeGlobalTable",
        "ListGlobalTables",
        "CreateBackup",
        "DeleteBackup",
        "DescribeBackup",
        "ListBackups",
        "RestoreTableFromBackup",
        "RestoreTableToPointInTime",
        "ExportTableToPointInTime",
        "ImportTable",
        "EnableKinesisStreamingDestination",
        "DisableKinesisStreamingDestination",
        "DescribeKinesisStreamingDestination",
        "UpdateKinesisStreamingDestination",
        "GetResourcePolicy",
        "PutResourcePolicy",
        "DeleteResourcePolicy",
        "SearchVectors",
        "DescribeTableReplicaAutoScaling",
        "UpdateTableReplicaAutoScaling",
    ];
    let h = Harness::new(service());
    for name in refused {
        assert!(!h.serves(name), "{name} must be refused");
    }
    for name in [
        "CreateTable",
        "PutItem",
        "Query",
        "TransactWriteItems",
        "UpdateContinuousBackups",
        "DescribeLimits",
        "ExecuteStatement",
        "BatchExecuteStatement",
        "ExecuteTransaction",
        "DescribeContributorInsights",
        "UpdateContributorInsights",
        "ListContributorInsights",
    ] {
        assert!(h.serves(name), "{name} must be served");
    }
}

fn call(h: &Harness<Dynamodb>, op: &str, body: serde_json::Value) -> aws_wire::service::Call {
    let mut c = h.call(h.op(op), &body).unwrap();
    c.caller = Caller {
        region: "eu-west-1".into(),
        account_id: "111122223333".into(),
        ..c.caller
    };
    c
}

#[test]
fn requests_are_described_as_actions_on_table_and_index_arns() {
    let h = Harness::new(service());
    let needs = |op: &str, body: serde_json::Value| crate::authz::needs(&call(&h, op, body));
    let n = needs(
        "PutItem",
        json!({"TableName": "orders", "Item": {"pk": {"S": "a"}}}),
    );
    assert_eq!(
        (n[0].action.as_str(), n[0].resource.as_str()),
        (
            "dynamodb:PutItem",
            "arn:aws:dynamodb:eu-west-1:111122223333:table/orders"
        )
    );
    let n = needs(
        "Query",
        json!({"TableName": "orders", "IndexName": "by-owner", "KeyConditionExpression": "pk = :p", "ExpressionAttributeValues": {":p": {"S": "a"}}}),
    );
    assert_eq!(
        n[0].resource,
        "arn:aws:dynamodb:eu-west-1:111122223333:table/orders/index/by-owner"
    );
    let n = needs("ListTables", json!({}));
    assert_eq!(n[0].resource, "*");
    let n = needs(
        "TransactWriteItems",
        json!({"TransactItems": [{"Put": {"TableName": "a1b", "Item": {"pk": {"S": "x"}}}}, {"ConditionCheck": {"TableName": "c1d", "Key": {"pk": {"S": "x"}}, "ConditionExpression": "attribute_exists(pk)"}}]}),
    );
    assert_eq!(
        n.iter().map(|x| x.action.as_str()).collect::<Vec<_>>(),
        ["dynamodb:PutItem", "dynamodb:ConditionCheckItem"]
    );
    let n = needs(
        "BatchGetItem",
        json!({"RequestItems": {"a1b": {"Keys": [{"pk": {"S": "x"}}]}, "c1d": {"Keys": [{"pk": {"S": "x"}}]}}}),
    );
    assert_eq!(n.len(), 2);
    let n = needs(
        "TagResource",
        json!({"ResourceArn": "arn:aws:dynamodb:eu-west-1:111122223333:table/orders", "Tags": [{"Key": "k", "Value": "v"}]}),
    );
    assert_eq!(
        n[0].resource,
        "arn:aws:dynamodb:eu-west-1:111122223333:table/orders"
    );
}

#[test]
fn another_region_is_another_world() {
    let svc = service();
    // Regions are kept apart by the Keyed actors; this checks the ARN of a table named in another region names nothing.
    let e = crate::req::table_name(
        "arn:aws:dynamodb:eu-west-1:000000000000:table/t",
        "us-east-1",
        "000000000000",
    )
    .unwrap_err();
    assert_eq!(e.code, "ResourceNotFoundException");
    assert_eq!(
        crate::req::table_name(
            "arn:aws:dynamodb:us-east-1:000000000000:table/t/index/i",
            "us-east-1",
            "000000000000"
        )
        .unwrap(),
        "t"
    );
    drop(svc);
}
