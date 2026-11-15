//! `doze-aws data` on API Gateway: one Storage for both APIs (through v1), emptying refused, and a reset removes the REST and HTTP
//! APIs alike and shrinks the file.

use aws_wire::service::Service;
use aws_wire::storage::{Prune, Storage, StorageError};
use serde_json::json;

use super::World;

#[tokio::test]
async fn one_storage_answers_for_both_apis_and_a_reset_removes_them_all() {
    let w = World::new();
    assert!(w.v1.storage().is_some());
    assert!(w.v2.storage().is_none(), "v2 shares v1's directory");
    for n in 0..150 {
        w.rest(
            "CreateRestApi",
            json!({"name": format!("r{n}"), "description": "d".repeat(300)}),
        )
        .await
        .unwrap();
        w.http(
            "CreateApi",
            json!({"name": format!("h{n}"), "protocolType": "HTTP", "description": "d".repeat(300)}),
        )
        .await
        .unwrap();
    }
    let usage = w.v1.usage().await.unwrap();
    assert_eq!(usage.files.len(), 1);
    assert!(usage.files[0].bytes > 300 * 300, "{usage:?}");
    assert!(matches!(
        w.v1.empty("us-east-1", "r0", &Prune::default()).await,
        Err(StorageError::Unsupported(_))
    ));

    let dry = w.v1.reset(true).await.unwrap();
    assert_eq!((dry.items, dry.freed), (300, 0));
    let done = w.v1.reset(false).await.unwrap();
    assert_eq!(done.items, 300);
    assert!(done.freed > 0, "the file shrank: {done:?}");
    let rest = w.rest("GetRestApis", json!({})).await.unwrap();
    assert!(rest["items"].as_array().is_none_or(Vec::is_empty), "{rest}");
    let http = w.http("GetApis", json!({})).await.unwrap();
    assert!(http["items"].as_array().is_none_or(Vec::is_empty), "{http}");
    w.rest("CreateRestApi", json!({"name": "r0"}))
        .await
        .unwrap();
}
