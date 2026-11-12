//! Event source mappings against the real SQS, Kinesis and DynamoDB beside Lambda on the bus: what a poller delivers, what it deletes
//! or retries, what the filters drop, and what reaches CloudWatch Logs.

use aws_core::base64;
use serde_json::{Value, json};

use super::{ACCOUNT, World, have};

const PRINTER: &[u8] = b"import json\ndef handler(event, context):\n    for r in event['Records']:\n        src = r['eventSource']\n        if src == 'aws:sqs':\n            print('GOT', src, r['body'])\n        elif src == 'aws:kinesis':\n            import base64\n            print('GOT', src, base64.b64decode(r['kinesis']['data']).decode())\n        else:\n            print('GOT', src, r['eventName'], json.dumps(r['dynamodb']['Keys']))\n    return {}\n";

const FAILING: &[u8] = b"def handler(event, context):\n    bad = [{'itemIdentifier': r['messageId']} for r in event['Records'] if r['body'] == 'bad']\n    print('GOT', len(event['Records']))\n    return {'batchItemFailures': bad}\n";

async fn queue(w: &World, name: &str, visibility: &str) -> (String, String) {
    let made = w
        .peer(
            "sqs",
            "CreateQueue",
            json!({"QueueName": name, "Attributes": {"VisibilityTimeout": visibility}}),
        )
        .await;
    let url = made["QueueUrl"].as_str().unwrap().to_owned();
    (url, format!("arn:aws:sqs:us-east-1:{ACCOUNT}:{name}"))
}

async fn logged(w: &World, function: &str, pattern: &str) -> Vec<String> {
    let group = format!("/aws/lambda/{function}");
    match w
        .peers
        .call(
            "test",
            &World::caller(),
            "logs",
            "FilterLogEvents",
            &json!({"logGroupName": group, "filterPattern": pattern}),
        )
        .await
    {
        Ok(r) => r["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["message"].as_str().unwrap().trim().to_owned())
            .collect(),
        Err(_) => Vec::new(),
    }
}

async fn counts(w: &World, url: &str) -> (u64, u64) {
    let got = w
        .peer("sqs", "GetQueueAttributes", json!({"QueueUrl": url, "AttributeNames": ["ApproximateNumberOfMessages", "ApproximateNumberOfMessagesNotVisible"]}))
        .await;
    let n = |k: &str| got["Attributes"][k].as_str().unwrap().parse().unwrap();
    (
        n("ApproximateNumberOfMessages"),
        n("ApproximateNumberOfMessagesNotVisible"),
    )
}

#[tokio::test]
async fn a_queue_drives_the_function_and_what_succeeds_is_deleted() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "p",
        "python3.12",
        "index.handler",
        &[("index.py", PRINTER, 0o644)],
    )
    .await;
    let (url, arn) = queue(&w, "q", "30").await;
    let made = w
        .ok(
            "CreateEventSourceMapping",
            json!({"FunctionName": "p", "EventSourceArn": arn, "BatchSize": 5}),
        )
        .await;
    assert_eq!(made["State"], "Creating");
    assert_eq!(
        w.ok("GetEventSourceMapping", json!({"UUID": made["UUID"]}))
            .await["State"],
        "Enabled"
    );
    for body in ["one", "two"] {
        w.peer(
            "sqs",
            "SendMessage",
            json!({"QueueUrl": url, "MessageBody": body}),
        )
        .await;
    }
    w.eventually("both messages consumed", || async {
        logged(&w, "p", "GOT").await.len() >= 2
    })
    .await;
    w.eventually("the queue is empty", || async {
        counts(&w, &url).await == (0, 0)
    })
    .await;
    let lines = logged(&w, "p", "GOT").await;
    assert!(
        lines.contains(&"GOT aws:sqs one".to_owned())
            && lines.contains(&"GOT aws:sqs two".to_owned()),
        "{lines:?}"
    );
    w.eventually("the result is noted", || async {
        w.ok("GetEventSourceMapping", json!({"UUID": made["UUID"]}))
            .await["LastProcessingResult"]
            == "OK"
    })
    .await;
    w.ok("DeleteEventSourceMapping", json!({"UUID": made["UUID"]}))
        .await;
    w.stop().await;
}

#[tokio::test]
async fn a_function_that_reports_failed_items_keeps_only_those_on_the_queue() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "p",
        "python3.12",
        "index.handler",
        &[("index.py", FAILING, 0o644)],
    )
    .await;
    let (url, arn) = queue(&w, "q", "30").await;
    for body in ["good", "bad", "fine"] {
        w.peer(
            "sqs",
            "SendMessage",
            json!({"QueueUrl": url, "MessageBody": body}),
        )
        .await;
    }
    w.ok("CreateEventSourceMapping", json!({"FunctionName": "p", "EventSourceArn": arn, "BatchSize": 10, "FunctionResponseTypes": ["ReportBatchItemFailures"]})).await;
    w.eventually("only the failed item is left", || async {
        counts(&w, &url).await == (0, 1)
    })
    .await;
    assert_eq!(logged(&w, "p", "GOT").await, ["GOT 3"]);
    w.stop().await;
}

#[tokio::test]
async fn a_filter_decides_what_reaches_the_function_and_the_rest_is_dropped() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "p",
        "python3.12",
        "index.handler",
        &[("index.py", PRINTER, 0o644)],
    )
    .await;
    let (url, arn) = queue(&w, "q", "30").await;
    let bad = w.err("CreateEventSourceMapping", json!({"FunctionName": "p", "EventSourceArn": arn, "FilterCriteria": {"Filters": [{"Pattern": "not json"}]}})).await;
    assert_eq!(bad.code, "InvalidParameterValueException");
    let filters = json!({"Filters": [{"Pattern": "{\"body\": {\"kind\": [\"order\"]}}"}]});
    w.ok(
        "CreateEventSourceMapping",
        json!({"FunctionName": "p", "EventSourceArn": arn, "FilterCriteria": filters}),
    )
    .await;
    for body in [r#"{"kind":"order"}"#, r#"{"kind":"other"}"#] {
        w.peer(
            "sqs",
            "SendMessage",
            json!({"QueueUrl": url, "MessageBody": body}),
        )
        .await;
    }
    w.eventually("both gone", || async { counts(&w, &url).await == (0, 0) })
        .await;
    let lines = logged(&w, "p", "GOT").await;
    assert_eq!(lines, [r#"GOT aws:sqs {"kind":"order"}"#]);
    w.stop().await;
}

#[tokio::test]
async fn a_queue_that_hides_messages_for_less_than_the_function_runs_is_refused() {
    let w = World::new();
    w.create(
        "p",
        "python3.12",
        "index.handler",
        &[("index.py", PRINTER, 0o644)],
    )
    .await;
    let (_, arn) = queue(&w, "short", "2").await;
    let e = w
        .err(
            "CreateEventSourceMapping",
            json!({"FunctionName": "p", "EventSourceArn": arn}),
        )
        .await;
    assert_eq!(
        e.message,
        "Queue visibility timeout: 2 seconds is less than Function timeout: 10 seconds"
    );
    let absent = w.err("CreateEventSourceMapping", json!({"FunctionName": "p", "EventSourceArn": format!("arn:aws:sqs:us-east-1:{ACCOUNT}:nope")})).await;
    assert!(absent.message.starts_with("Error occurred while ReceiveMessage. SQS Error Code: AWS.SimpleQueueService.NonExistentQueue."), "{}", absent.message);
    let kafka = w.err("CreateEventSourceMapping", json!({"FunctionName": "p", "EventSourceArn": "arn:aws:kafka:us-east-1:111122223333:cluster/c/1"})).await;
    assert_eq!(kafka.status, 501);
    w.stop().await;
}

#[tokio::test]
async fn a_kinesis_stream_and_a_dynamodb_stream_drive_the_function() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "p",
        "python3.12",
        "index.handler",
        &[("index.py", PRINTER, 0o644)],
    )
    .await;
    w.peer(
        "kinesis",
        "CreateStream",
        json!({"StreamName": "s", "ShardCount": 1}),
    )
    .await;
    let described = w
        .peer(
            "kinesis",
            "DescribeStreamSummary",
            json!({"StreamName": "s"}),
        )
        .await;
    let stream_arn = described["StreamDescriptionSummary"]["StreamARN"]
        .as_str()
        .unwrap()
        .to_owned();
    w.peer(
        "kinesis",
        "PutRecord",
        json!({"StreamName": "s", "PartitionKey": "k", "Data": base64::encode(b"before")}),
    )
    .await;
    w.ok("CreateEventSourceMapping", json!({"FunctionName": "p", "EventSourceArn": stream_arn, "StartingPosition": "TRIM_HORIZON", "BatchSize": 10})).await;
    w.peer(
        "kinesis",
        "PutRecord",
        json!({"StreamName": "s", "PartitionKey": "k", "Data": base64::encode(b"after")}),
    )
    .await;
    w.eventually("both records", || async {
        logged(&w, "p", "aws:kinesis").await.len() >= 2
    })
    .await;
    assert_eq!(
        logged(&w, "p", "aws:kinesis").await,
        ["GOT aws:kinesis before", "GOT aws:kinesis after"]
    );

    let table = w
        .peer("dynamodb", "CreateTable", json!({"TableName": "tbl", "BillingMode": "PAY_PER_REQUEST",
            "AttributeDefinitions": [{"AttributeName": "id", "AttributeType": "S"}], "KeySchema": [{"AttributeName": "id", "KeyType": "HASH"}],
            "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES"}}))
        .await;
    let ddb_arn = table["TableDescription"]["LatestStreamArn"]
        .as_str()
        .unwrap()
        .to_owned();
    w.ok(
        "CreateEventSourceMapping",
        json!({"FunctionName": "p", "EventSourceArn": ddb_arn, "StartingPosition": "LATEST"}),
    )
    .await;
    w.peer(
        "dynamodb",
        "PutItem",
        json!({"TableName": "tbl", "Item": {"id": {"S": "a"}}}),
    )
    .await;
    w.eventually("the item change", || async {
        !logged(&w, "p", "aws:dynamodb").await.is_empty()
    })
    .await;
    assert_eq!(
        logged(&w, "p", "aws:dynamodb").await,
        [r#"GOT aws:dynamodb INSERT {"id": {"S": "a"}}"#]
    );
    w.stop().await;
}

#[tokio::test]
async fn a_function_logs_in_the_shape_aws_writes() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "p",
        "python3.12",
        "index.handler",
        &[(
            "index.py",
            b"def handler(e, c):\n    print('hello')\n    return 1\n",
            0o644,
        )],
    )
    .await;
    w.invoke("p", &json!({})).await;
    let lines = logged(&w, "p", "").await;
    assert!(
        lines[0].starts_with("START RequestId: ") && lines[0].ends_with(" Version: $LATEST"),
        "{lines:?}"
    );
    assert_eq!(lines[1], "hello");
    assert!(lines[2].starts_with("END RequestId: "), "{lines:?}");
    assert!(
        lines[3].starts_with("REPORT RequestId: ")
            && lines[3].contains("\tBilled Duration: ")
            && lines[3].contains("\tMemory Size: 128 MB"),
        "{lines:?}"
    );
    let streams = w
        .peer(
            "logs",
            "DescribeLogStreams",
            json!({"logGroupName": "/aws/lambda/p"}),
        )
        .await;
    let name = streams["logStreams"][0]["logStreamName"].as_str().unwrap();
    assert!(name.contains("/[$LATEST]"), "{name}");
    let _: Value = json!(null);
    w.stop().await;
}
