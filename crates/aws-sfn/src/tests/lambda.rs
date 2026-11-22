//! Tasks that invoke a Lambda function. The function is a stand-in that answers the way `aws-lambda` does on the peer bus: the
//! payload is a base64 member `Payload` of the JSON answer (there is no body stream), and a function that threw says
//! `FunctionError: Unhandled` with its error document as that payload.

use std::sync::Arc;

use aws_model_lambda::meta::META;
use aws_model_lambda::tables::TABLES;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::rest::{RestReply, RestService, Upload};
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use serde_json::{Value, json};

use super::{World, output};

/// A Lambda that runs two functions: `echo` answers `{"echoed": <input>}` and `boom` throws a `Boom` error.
struct Stub;

impl Service for Stub {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn call(&self, _call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async { Err(ApiError::internal("the stand-in answers over REST only")) })
    }

    fn rest(&self) -> Option<&dyn RestService> {
        Some(self)
    }
}

impl RestService for Stub {
    fn call_rest(
        &self,
        call: Call,
        _upload: Option<Box<dyn Upload>>,
    ) -> BoxFuture<'_, Result<RestReply, ApiError>> {
        Box::pin(async move {
            let request: Value = serde_json::from_str(&call.body).unwrap();
            let input = request["Payload"]
                .as_str()
                .and_then(aws_core::base64::decode)
                .map(|b| String::from_utf8(b).unwrap())
                .unwrap_or_default();
            let input: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
            let mut json = json!({"StatusCode": 200, "ExecutedVersion": "$LATEST"});
            let answer = if request["FunctionName"].as_str().unwrap().ends_with("boom") {
                json["FunctionError"] = json!("Unhandled");
                json!({"errorType": "Boom", "errorMessage": "it blew up"})
            } else {
                json!({"echoed": input})
            };
            json["Payload"] = json!(aws_core::base64::encode(answer.to_string().as_bytes()));
            Ok(RestReply {
                json,
                ..RestReply::default()
            })
        })
    }
}

fn world() -> World {
    let w = World::new();
    w.peers.register(Arc::new(Stub));
    w
}

fn function(name: &str) -> String {
    format!("arn:aws:lambda:us-east-1:000000000000:function:{name}")
}

#[tokio::test]
async fn lambda_invoke_returns_the_functions_answer_as_payload() {
    let w = world();
    let m = w
        .machine(
            "invoke",
            json!({"StartAt": "Call", "States": {
                "Call": {"Type": "Task", "Resource": "arn:aws:states:::lambda:invoke",
                    "Parameters": {"FunctionName": function("echo"), "Payload": {"n.$": "$.n"}}, "End": true}}}),
        )
        .await;
    let d = w.run(&m, json!({"n": 7})).await;
    assert_eq!(d["status"], "SUCCEEDED", "{d}");
    let out = output(&d);
    assert_eq!(out["Payload"], json!({"echoed": {"n": 7}}));
    assert_eq!(out["StatusCode"], 200);
    assert_eq!(out["ExecutedVersion"], "$LATEST");
    w.stop().await;
}

#[tokio::test]
async fn a_bare_function_arn_gets_the_input_and_the_answer() {
    let w = world();
    let m = w
        .machine(
            "bare",
            json!({"StartAt": "Call", "States": {
                "Call": {"Type": "Task", "Resource": function("echo"), "End": true}}}),
        )
        .await;
    let d = w.run(&m, json!({"who": "me"})).await;
    assert_eq!(d["status"], "SUCCEEDED", "{d}");
    assert_eq!(output(&d), json!({"echoed": {"who": "me"}}));
    w.stop().await;
}

#[tokio::test]
async fn a_function_that_throws_fails_the_task_by_its_error_type() {
    let w = world();
    for resource in ["bare", "invoke"] {
        let task = if resource == "bare" {
            json!({"Type": "Task", "Resource": function("boom"), "End": true})
        } else {
            json!({"Type": "Task", "Resource": "arn:aws:states:::lambda:invoke",
                "Parameters": {"FunctionName": function("boom"), "Payload": {}}, "End": true})
        };
        let m = w
            .machine(
                resource,
                json!({"StartAt": "Call", "States": {"Call": task}}),
            )
            .await;
        let d = w.run(&m, json!({})).await;
        assert_eq!(d["status"], "FAILED", "{resource}: {d}");
        assert_eq!(d["error"], "Boom", "{resource}: {d}");
        let cause: Value = serde_json::from_str(d["cause"].as_str().unwrap()).unwrap();
        assert_eq!(
            cause,
            json!({"errorType": "Boom", "errorMessage": "it blew up"}),
            "{resource}"
        );
    }
    w.stop().await;
}
