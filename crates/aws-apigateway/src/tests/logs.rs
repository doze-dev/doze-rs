//! What a stage writes to CloudWatch Logs: the execution narrative of a REST method whose settings ask for it (`INFO`, `ERROR`, with
//! data trace), and the access-log line in the stage's format.

use aws_wire::service::Caller;
use serde_json::{Value, json};

use super::World;

async fn events(peers: &aws_peers::Peers, group: &str) -> Vec<String> {
    let caller = Caller {
        access_key_id: "test".into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: String::new(),
        service: None,
        session_token: None,
    };
    let out = peers
        .call(
            "test",
            &caller,
            "logs",
            "FilterLogEvents",
            &json!({"logGroupName": group}),
        )
        .await
        .unwrap_or_default();
    out["events"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["message"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

async fn stage_with(w: &World, api: &str, patches: Value) {
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "dev"}),
    )
    .await
    .unwrap();
    w.rest("UpdateAccount", json!({"patchOperations": [{"op": "replace", "path": "/cloudwatchRoleArn", "value": "arn:aws:iam::000000000000:role/cw"}]}))
        .await
        .unwrap();
    w.rest(
        "UpdateStage",
        json!({"restApiId": api, "stageName": "dev", "patchOperations": patches}),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn an_info_stage_writes_the_execution_narrative_and_data_trace_adds_the_bodies() {
    let (w, peers) = World::with_logs();
    let (api, rid) = w.rest_api_with("ping").await;
    w.mock(&api, &rid, r#"{"statusCode": 200}"#, r#"{"pong": true}"#)
        .await;
    stage_with(
        &w,
        &api,
        json!([
            {"op": "replace", "path": "/*/*/logging/loglevel", "value": "INFO"},
            {"op": "replace", "path": "/*/*/logging/dataTrace", "value": "true"},
        ]),
    )
    .await;
    let r = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/dev/ping?x=1"),
            "",
            &[],
        )
        .await;
    assert_eq!(r.status, 200);
    let lines = events(&peers, &format!("API-Gateway-Execution-Logs_{api}/dev")).await;
    let text = lines.join("\n");
    assert!(lines.iter().all(|l| l.starts_with('(')), "{text}");
    for want in [
        "Starting execution for request:",
        "HTTP Method: GET, Resource Path: /ping",
        "Method request query string: {x=1}",
        "Method response body after transformations: {\"pong\": true}",
        "Successfully completed execution",
        "Method completed with status: 200",
    ] {
        assert!(text.contains(want), "{want} not in:\n{text}");
    }
}

#[tokio::test]
async fn an_error_stage_writes_only_failures_and_an_access_log_line_is_written_in_its_format() {
    let (w, peers) = World::with_logs();
    let (api, rid) = w.rest_api_with("ping").await;
    w.mock(&api, &rid, r#"{"statusCode": 200}"#, "{}").await;
    let caller = Caller {
        access_key_id: "test".into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: String::new(),
        service: None,
        session_token: None,
    };
    peers
        .call(
            "test",
            &caller,
            "logs",
            "CreateLogGroup",
            &json!({"logGroupName": "access"}),
        )
        .await
        .unwrap();
    stage_with(&w, &api, json!([
        {"op": "replace", "path": "/*/*/logging/loglevel", "value": "ERROR"},
        {"op": "replace", "path": "/accessLogSettings/destinationArn", "value": "arn:aws:logs:us-east-1:000000000000:log-group:access"},
        {"op": "replace", "path": "/accessLogSettings/format", "value": "$context.httpMethod $context.resourcePath $context.status $context.error.messageString"},
    ]))
    .await;
    assert_eq!(
        w.web("GET", &format!("/_aws/execute-api/{api}/dev/ping"), "", &[])
            .await
            .status,
        200
    );
    assert_eq!(
        w.web("GET", &format!("/_aws/execute-api/{api}/dev/nope"), "", &[])
            .await
            .status,
        403
    );
    let lines = events(&peers, &format!("API-Gateway-Execution-Logs_{api}/dev")).await;
    assert_eq!(
        lines.len(),
        0,
        "ERROR writes nothing for a success, and the unmatched path has no method: {lines:?}"
    );
    let access = events(&peers, "access").await;
    assert_eq!(
        access.first().map(String::as_str),
        Some("GET /ping 200 -"),
        "{access:?}"
    );
    assert!(
        access
            .get(1)
            .is_some_and(|l| l.contains("403 \"Missing Authentication Token\"")),
        "{access:?}"
    );
}
