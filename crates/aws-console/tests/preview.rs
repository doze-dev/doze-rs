//! `POST /api/v1/preview`: the call a form would make, written from the model, for each way a service speaks: a JSON protocol
//! (SQS), a query protocol (SNS), REST with XML (S3) and REST with JSON (Lambda). Nothing is called.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use support::{keys, ts_keys};
use world::{World, world};

async fn preview(w: &World, service: &str, operation: &str, input: &Value) -> (u16, Value) {
    w.send(
        "POST",
        "/api/v1/preview",
        &json!({"service": service, "operation": operation, "input": input}),
    )
    .await
}

#[tokio::test]
async fn a_json_protocol_call_is_its_body_and_its_commands() {
    let w = world("preview-json", Mode::Soft, &[]);
    let input = json!({"QueueName": "orders", "Attributes": {"VisibilityTimeout": "45"}, "tags": {"env": "dev"}});
    let (status, p) = preview(&w, "sqs", "CreateQueue", &input).await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(keys(&p), ts_keys("Preview"));
    assert_eq!(p["json"], input);
    assert_eq!(p["missing"], json!([]));
    let cli = p["cli"].as_str().unwrap();
    assert!(
        cli.starts_with("aws --endpoint-url http://127.0.0.1:4566 --region us-east-1 sqs create-queue --queue-name orders"),
        "{cli}"
    );
    assert!(
        cli.contains(r#"--attributes '{"VisibilityTimeout":"45"}'"#),
        "{cli}"
    );
    assert!(cli.contains(r#"--tags '{"env":"dev"}'"#), "{cli}");
    let boto = p["boto3"].as_str().unwrap();
    assert!(boto.contains("import boto3"), "{boto}");
    assert!(boto.contains(r#"sqs = boto3.client("#), "{boto}");
    assert!(
        boto.contains(r#"endpoint_url="http://127.0.0.1:4566""#),
        "{boto}"
    );
    assert!(boto.contains("sqs.create_queue("), "{boto}");
    assert!(boto.contains(r#"QueueName="orders""#), "{boto}");
    assert!(
        boto.contains(r#"Attributes={"VisibilityTimeout": "45"}"#),
        "{boto}"
    );
}

#[tokio::test]
async fn a_required_member_that_is_missing_is_named_and_the_call_is_still_written() {
    let w = world("preview-missing", Mode::Soft, &[]);
    let (status, p) = preview(&w, "sqs", "CreateQueue", &json!({})).await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["missing"], json!(["QueueName"]));
    assert!(p["cli"].as_str().unwrap().ends_with("sqs create-queue"));
    // No input at all is the same as an empty one.
    let (status, p) = w
        .send(
            "POST",
            "/api/v1/preview",
            &json!({"service": "sqs", "operation": "CreateQueue"}),
        )
        .await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["missing"], json!(["QueueName"]));
}

#[tokio::test]
async fn a_query_protocol_call_is_its_form_members_flattened_as_the_service_reads_them() {
    let w = world("preview-query", Mode::Soft, &[]);
    let input = json!({"Name": "order-events", "Attributes": {"DisplayName": "orders", "FifoTopic": "false"},
        "Tags": [{"Key": "env", "Value": "dev"}, {"Key": "team", "Value": "a b"}]});
    let (status, p) = preview(&w, "sns", "CreateTopic", &input).await;
    assert_eq!(status, 200, "{p}");
    let form = &p["json"];
    assert_eq!(form["Action"], "CreateTopic");
    assert_eq!(form["Version"], "2010-03-31");
    assert_eq!(form["Name"], "order-events");
    assert_eq!(form["Attributes.entry.1.key"], "DisplayName");
    assert_eq!(form["Attributes.entry.1.value"], "orders");
    assert_eq!(form["Attributes.entry.2.key"], "FifoTopic");
    assert_eq!(form["Tags.member.1.Key"], "env");
    assert_eq!(form["Tags.member.2.Value"], "a b");
    // The form is what the service reads: decoded by its own reader, it is the input again.
    let cli = p["cli"].as_str().unwrap();
    assert!(
        cli.contains(" sns create-topic --name order-events "),
        "{cli}"
    );
    assert!(cli.contains("--tags"), "{cli}");
    assert!(p["boto3"].as_str().unwrap().contains("sns.create_topic("));
}

#[tokio::test]
async fn a_rest_call_is_the_members_that_are_not_in_its_address() {
    let w = world("preview-rest", Mode::Soft, &[]);
    // rest-xml: the bucket is in the address, the tag set (the payload member, `Tagging` in the XML) is the body.
    let (status, p) = preview(
        &w,
        "s3",
        "PutBucketTagging",
        &json!({"Bucket": "b", "Tagging": {"TagSet": [{"Key": "k", "Value": "v"}]}}),
    )
    .await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["json"], json!({"TagSet": [{"Key": "k", "Value": "v"}]}));
    assert!(
        p["cli"]
            .as_str()
            .unwrap()
            .contains(" s3api put-bucket-tagging --bucket b ")
    );
    assert!(p["boto3"].as_str().unwrap().contains("s3 = boto3.client("));
    assert!(
        p["boto3"]
            .as_str()
            .unwrap()
            .contains("s3.put_bucket_tagging(")
    );
    // rest-json: the function is in the address, the payload is the body.
    let (status, p) = preview(
        &w,
        "lambda",
        "Invoke",
        &json!({"FunctionName": "echo", "Payload": "{\"n\":1}", "LogType": "Tail"}),
    )
    .await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["json"], json!("{\"n\":1}"));
    assert!(
        p["cli"]
            .as_str()
            .unwrap()
            .contains(" lambda invoke --function-name echo --log-type Tail --payload ")
    );
    // `lambda` is a Python keyword: the client is not held in a variable of that name.
    assert!(
        p["boto3"]
            .as_str()
            .unwrap()
            .contains("lambda_client = boto3.client("),
        "{p}"
    );
    assert_eq!(p["missing"], json!([]));
    // A service is also found by its name in --services.
    let (status, p) = preview(
        &w,
        "stepfunctions",
        "StartExecution",
        &json!({"stateMachineArn": "arn:x"}),
    )
    .await;
    assert_eq!(status, 200, "{p}");
    assert!(
        p["cli"]
            .as_str()
            .unwrap()
            .contains(" stepfunctions start-execution --state-machine-arn arn:x")
    );
    assert!(
        p["boto3"]
            .as_str()
            .unwrap()
            .contains("stepfunctions.start_execution(")
    );
    assert_eq!(p["missing"], json!([]));
}

#[tokio::test]
async fn a_call_doze_aws_does_not_do_is_not_built() {
    let w = world("preview-not-built", Mode::Soft, &[]);
    for (service, operation) in [("sqs", "Frobnicate"), ("nothing", "CreateQueue")] {
        let (status, p) = preview(&w, service, operation, &json!({})).await;
        assert_eq!(
            (status, p["code"].as_str()),
            (404, Some("NOT_BUILT")),
            "{p}"
        );
    }
    // An operation that is in AWS's model and not built here.
    let sns = w
        .state
        .facts
        .peers
        .as_ref()
        .unwrap()
        .targets()
        .into_iter()
        .find(|(n, _)| *n == "sns")
        .unwrap()
        .1;
    let tables = sns.tables();
    let unbuilt = (0..tables.n_ops())
        .find(|i| !sns.serves(*i))
        .and_then(|i| tables.op(i))
        .map(|o| o.name.to_owned());
    if let Some(name) = unbuilt {
        let (status, p) = preview(&w, "sns", &name, &json!({})).await;
        assert_eq!(
            (status, p["code"].as_str()),
            (404, Some("NOT_BUILT")),
            "{name}: {p}"
        );
    }
    // A body that is not a preview request is refused.
    let (status, p) = w.send("POST", "/api/v1/preview", &json!("sqs")).await;
    assert_eq!((status, p["code"].as_str()), (400, Some("BAD_REQUEST")));
}

#[tokio::test]
async fn a_preview_calls_nothing() {
    let w = world("preview-pure", Mode::Soft, &[]);
    let (status, _) = preview(&w, "sqs", "CreateQueue", &json!({"QueueName": "never"})).await;
    assert_eq!(status, 200);
    let (_, queues) = w.get("/api/v1/resources/sqs/queues").await;
    assert_eq!(queues["rows"], json!([]));
    assert!(
        !w.live().iter().any(|l| l.contains("CreateQueue")),
        "{:?}",
        w.live()
    );
}

#[tokio::test]
async fn the_form_a_preview_writes_is_read_back_by_the_services_own_decoder() {
    let w = world("preview-roundtrip", Mode::Soft, &[]);
    let input = json!({"Name": "order-events", "Attributes": {"DisplayName": "orders"},
        "Tags": [{"Key": "env", "Value": "dev"}, {"Key": "team", "Value": "a&b=c"}]});
    let (_, p) = preview(&w, "sns", "CreateTopic", &input).await;
    // The form as a body (percent-encoded), read by the wire layer as the service reads it.
    let body: Vec<String> = p["json"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| {
            format!(
                "{k}={}",
                v.as_str()
                    .unwrap()
                    .replace('%', "%25")
                    .replace('&', "%26")
                    .replace('=', "%3D")
                    .replace(' ', "+")
            )
        })
        .collect();
    let form = aws_wire::query::parse_form(&body.join("&"));
    let sns = w
        .state
        .facts
        .peers
        .as_ref()
        .unwrap()
        .targets()
        .into_iter()
        .find(|(n, _)| *n == "sns")
        .unwrap()
        .1;
    let tables = sns.tables();
    let op = tables.op(tables.find_op("CreateTopic").unwrap()).unwrap();
    assert_eq!(
        aws_wire::query::to_json(tables, op.input.unwrap(), &form),
        input
    );
}

#[tokio::test]
async fn a_timestamp_written_as_text_is_one_word_on_the_command_line() {
    let w = world("preview-timestamp", Mode::Soft, &[]);
    let input = json!({
        "ReplayName": "redo",
        "EventSourceArn": "arn:aws:events:us-east-1:000000000000:archive/a",
        "EventStartTime": "2026-11-07T00:00:00Z",
        "EventEndTime": "2026-11-08T00:00:00Z",
        "Destination": {"Arn": "arn:aws:events:us-east-1:000000000000:event-bus/larder"}
    });
    let (status, p) = preview(&w, "events", "StartReplay", &input).await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["json"]["EventStartTime"], "2026-11-07T00:00:00Z");
    let cli = p["cli"].as_str().unwrap();
    assert!(
        cli.contains(" --event-start-time 2026-11-07T00:00:00Z"),
        "{cli}"
    );
    assert!(!cli.contains("'\"2026"), "{cli}");
    let boto = p["boto3"].as_str().unwrap();
    assert!(
        boto.contains(r#"EventStartTime="2026-11-07T00:00:00Z""#),
        "{boto}"
    );
}
