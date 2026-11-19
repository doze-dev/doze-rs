//! Stage apigw-ops, continued: usage (metered, enforced, reported and granted), imported API keys, and the test invocations.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use serde_json::{Value, json};

use super::{World, body};

/// A deployed `GET /p` (MOCK) that requires a key, a key and a plan with `quota` and `throttle`: `(api, key id, key value, plan)`.
async fn metered(w: &World, quota: Value, throttle: Value) -> (String, String, String, String) {
    let (api, rid) = w.rest_api_with("p").await;
    w.mock(&api, &rid, "{\"statusCode\": 200}", "{\"ok\": true}")
        .await;
    w.rest(
        "UpdateMethod",
        json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET",
        "patchOperations": [{"op": "replace", "path": "/apiKeyRequired", "value": "true"}]}),
    )
    .await
    .unwrap();
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "dev"}),
    )
    .await
    .unwrap();
    let value = "abcdefghijklmnopqrstuvwxyz0123".to_owned();
    let key = w
        .rest(
            "CreateApiKey",
            json!({"name": "k", "enabled": true, "value": value}),
        )
        .await
        .unwrap();
    let key_id = key["id"].as_str().unwrap().to_owned();
    let mut plan = json!({"name": "p", "apiStages": [{"apiId": api, "stage": "dev"}]});
    if !quota.is_null() {
        plan["quota"] = quota;
    }
    if !throttle.is_null() {
        plan["throttle"] = throttle;
    }
    let plan = w.rest("CreateUsagePlan", plan).await.unwrap();
    let plan_id = plan["id"].as_str().unwrap().to_owned();
    w.rest(
        "CreateUsagePlanKey",
        json!({"usagePlanId": plan_id, "keyId": key_id, "keyType": "API_KEY"}),
    )
    .await
    .unwrap();
    (api, key_id, value, plan_id)
}

fn today() -> String {
    let days = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
        / 86_400;
    crate::usage::day_text(days)
}

#[tokio::test]
async fn usage_is_counted_the_quota_enforced_and_a_grant_extends_it() {
    let w = World::new();
    let (api, key_id, value, plan) =
        metered(&w, json!({"limit": 2, "period": "DAY"}), Value::Null).await;
    let path = format!("/_aws/execute-api/{api}/dev/p");
    let headers = [("x-api-key", value.as_str())];
    let call = || w.web("GET", &path, "", &headers);
    assert_eq!(call().await.status, 200);
    assert_eq!(call().await.status, 200);
    let over = call().await;
    assert_eq!(over.status, 429);
    assert_eq!(body(&over), json!({"message": "Limit Exceeded"}));
    let day = today();
    let usage = w
        .rest(
            "GetUsage",
            json!({"usagePlanId": plan, "startDate": day, "endDate": day}),
        )
        .await
        .unwrap();
    // `items` is `values` on the wire (the model's jsonName).
    assert_eq!(usage["values"][&key_id], json!([[2, 0]]));
    let granted = w
        .rest("UpdateUsage", json!({"usagePlanId": plan, "keyId": key_id, "patchOperations": [{"op": "replace", "path": "/remaining", "value": "1"}]}))
        .await
        .unwrap();
    assert_eq!(granted["values"][&key_id], json!([[2, 1]]));
    assert_eq!(call().await.status, 200);
    assert_eq!(call().await.status, 429);
    let bad = w
        .rest(
            "GetUsage",
            json!({"usagePlanId": plan, "startDate": "2026-13-01", "endDate": day}),
        )
        .await
        .unwrap_err();
    assert_eq!(bad.code, "BadRequestException");
    let no_quota = w
        .rest("UpdateUsage", json!({"usagePlanId": plan, "keyId": "nope", "patchOperations": [{"op": "replace", "path": "/remaining", "value": "1"}]}))
        .await
        .unwrap_err();
    assert_eq!(no_quota.code, "NotFoundException");
}

#[tokio::test]
async fn a_throttle_refuses_past_its_burst() {
    let w = World::new();
    let (api, _, value, _) = metered(
        &w,
        Value::Null,
        json!({"burstLimit": 1, "rateLimit": 0.001}),
    )
    .await;
    let path = format!("/_aws/execute-api/{api}/dev/p");
    assert_eq!(
        w.web("GET", &path, "", &[("x-api-key", value.as_str())])
            .await
            .status,
        200
    );
    let refused = w
        .web("GET", &path, "", &[("x-api-key", value.as_str())])
        .await;
    assert_eq!(refused.status, 429);
    assert_eq!(body(&refused), json!({"message": "Too Many Requests"}));
}

#[tokio::test]
async fn api_keys_are_imported_from_csv() {
    let w = World::new();
    let plan = w
        .rest("CreateUsagePlan", json!({"name": "p"}))
        .await
        .unwrap();
    let pid = plan["id"].as_str().unwrap();
    let csv = format!(
        "Enabled,Name,key,UsageplanIds,extra\ntrue,First,apikey1234abcdefghij0123456789,\"{pid}\",x\nfalse,Second,short,,\n"
    );
    let out = w
        .rest(
            "ImportApiKeys",
            json!({"body": aws_core::base64::encode(csv.as_bytes()), "format": "csv"}),
        )
        .await
        .unwrap();
    assert_eq!(out["ids"].as_array().unwrap().len(), 1);
    assert_eq!(out["warnings"].as_array().unwrap().len(), 1);
    let keys = w
        .rest("GetUsagePlanKeys", json!({"usagePlanId": pid}))
        .await
        .unwrap();
    assert_eq!(keys["item"][0]["name"], "First");
    let strict = w
        .rest("ImportApiKeys", json!({"body": aws_core::base64::encode(csv.as_bytes()), "format": "csv", "failOnWarnings": true}))
        .await
        .unwrap_err();
    assert_eq!(strict.code, "BadRequestException");
    assert_eq!(
        strict.message,
        "Warnings found during import:\n\tSkipped invalid record: 2. API Key value should be at least 20 characters"
    );
    let no_header = w
        .rest(
            "ImportApiKeys",
            json!({"body": aws_core::base64::encode(b"a,b\n1,2\n"), "format": "csv"}),
        )
        .await
        .unwrap_err();
    assert_eq!(no_header.code, "BadRequestException");
}

#[tokio::test]
async fn test_invoke_method_runs_the_live_api_and_logs_it() {
    let w = World::new();
    let (api, rid) = w.rest_api_with("{id}").await;
    w.mock(&api, &rid, "{\"statusCode\": 200}", "{\"id\": \"$input.params('id')\", \"stage\": \"$context.stage\", \"q\": \"$input.params('q')\"}")
        .await;
    // Never deployed: a test runs the API as it is.
    let out = w
        .rest("TestInvokeMethod", json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET", "pathWithQueryString": "/42?q=x",
            "headers": {"Accept": "application/json"}}))
        .await
        .unwrap();
    assert_eq!(out["status"], 200);
    let b: Value = serde_json::from_str(out["body"].as_str().unwrap()).unwrap();
    assert_eq!(
        b,
        json!({"id": "42", "stage": "test-invoke-stage", "q": "x"})
    );
    assert_eq!(out["headers"]["Content-Type"], "application/json");
    assert_eq!(
        out["multiValueHeaders"]["Content-Type"],
        json!(["application/json"])
    );
    let log = out["log"].as_str().unwrap();
    assert!(log.starts_with("Execution log for request "), "{log}");
    assert!(
        log.contains(" UTC 20") && log.contains(" : Starting execution for request: "),
        "{log}"
    );
    assert!(
        log.contains("HTTP Method: GET, Resource Path: /42"),
        "{log}"
    );
    assert!(
        log.trim_end()
            .ends_with("Method completed with status: 200"),
        "{log}"
    );
    assert!(!log.contains("Usage Plan"), "{log}");
    assert!(out["latency"].as_i64().is_some());
    let missing = w
        .rest(
            "TestInvokeMethod",
            json!({"restApiId": api, "resourceId": rid, "httpMethod": "POST"}),
        )
        .await
        .unwrap_err();
    assert_eq!(missing.code, "NotFoundException");
}

#[tokio::test]
async fn test_invoke_authorizer_answers_what_the_client_would_get() {
    let w = World::new();
    let api = w.rest("CreateRestApi", json!({"name": "a"})).await.unwrap();
    let id = api["id"].as_str().unwrap();
    let auth = w
        .rest("CreateAuthorizer", json!({"restApiId": id, "name": "c", "type": "COGNITO_USER_POOLS",
            "providerARNs": ["arn:aws:cognito-idp:us-east-1:000000000000:userpool/us-east-1_abc"], "identitySource": "method.request.header.Authorization"}))
        .await
        .unwrap();
    let out = w
        .rest("TestInvokeAuthorizer", json!({"restApiId": id, "authorizerId": auth["id"], "headers": {"Authorization": "not-a-token"}}))
        .await
        .unwrap();
    assert_eq!(out["clientStatus"], 401);
    assert_eq!(
        out["authorization"],
        json!({"Authorization": ["not-a-token"]})
    );
    let log = out["log"].as_str().unwrap();
    assert!(
        log.contains(&format!(
            "Starting authorizer: {} for request: ",
            auth["id"].as_str().unwrap()
        )),
        "{log}"
    );
    assert!(log.contains("Unauthorized request: "), "{log}");
}
