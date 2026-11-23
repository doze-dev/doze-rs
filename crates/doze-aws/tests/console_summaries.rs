//! What the record says each call did, through the real binary: the services answer, the gateway reads the answer, and the
//! console's `summary` is a few true words (`2 messages`, `sent 5 B`, `delivered to 2`). A call that failed has none, and an
//! operation the table does not know has none.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_doze-aws");

fn start(dir: &Path) -> (Child, String, String) {
    let mut child = Command::new(BIN)
        .args(["--listen", "127.0.0.1:0", "--console", "127.0.0.1:0"])
        .args([
            "--services",
            "sqs,sns,s3,dynamodb,kinesis,eventbridge,logs,cloudwatch,iam,sts,stepfunctions",
        ])
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let (mut endpoint, mut console) = (String::new(), String::new());
    for line in lines.by_ref() {
        let line = line.unwrap();
        if let Some((label, value)) = line.trim().split_once(' ') {
            match label {
                "endpoint" => value.trim().clone_into(&mut endpoint),
                "console" => value.trim().clone_into(&mut console),
                _ => {}
            }
        }
        if line.contains("when something looks wrong") {
            break;
        }
    }
    std::thread::spawn(move || lines.for_each(drop));
    (child, endpoint, console)
}

fn request(url: &str, head: &str, body: &str) -> String {
    let host = url.trim_start_matches("http://");
    let mut s = TcpStream::connect(host).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "{head}\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

fn signed(service: &str) -> String {
    format!(
        "X-Amz-Date: 20261114T000000Z\r\nAuthorization: AWS4-HMAC-SHA256 Credential=AKIDSUM/20261114/us-east-1/{service}/aws4_request, \
         SignedHeaders=host;x-amz-date, Signature=00"
    )
}

/// A JSON-protocol call: `prefix.action`, the signing name and content type of the service.
fn json(
    endpoint: &str,
    (service, prefix, version): (&str, &str, &str),
    action: &str,
    body: &str,
) -> String {
    let head = format!(
        "POST / HTTP/1.1\r\nX-Amz-Target: {prefix}.{action}\r\nContent-Type: application/x-amz-json-{version}\r\n{}",
        signed(service)
    );
    ok(&request(endpoint, &head, body))
}

const SQS: (&str, &str, &str) = ("sqs", "AmazonSQS", "1.0");
const DDB: (&str, &str, &str) = ("dynamodb", "DynamoDB_20120810", "1.0");
const KINESIS: (&str, &str, &str) = ("kinesis", "Kinesis_20131202", "1.1");
const EVENTS: (&str, &str, &str) = ("events", "AWSEvents", "1.1");
const CLOUDWATCH: (&str, &str, &str) = ("monitoring", "GraniteServiceVersion20100801", "1.0");
const LOGS: (&str, &str, &str) = ("logs", "Logs_20140328", "1.1");

fn query(endpoint: &str, service: &str, form: &str) -> String {
    let head = format!(
        "POST / HTTP/1.1\r\nContent-Type: application/x-www-form-urlencoded\r\n{}",
        signed(service)
    );
    ok(&request(endpoint, &head, form))
}

fn s3(endpoint: &str, method: &str, path: &str, body: &str) -> String {
    let head = format!("{method} {path} HTTP/1.1\r\n{}", signed("s3"));
    ok(&request(endpoint, &head, body))
}

fn ok(made: &str) -> String {
    assert!(made.starts_with("HTTP/1.1 200"), "{made}");
    made.to_owned()
}

fn api(console: &str, path: &str) -> Value {
    let answer = request(console, &format!("GET {path} HTTP/1.1\r\nX-Console: 1"), "");
    let body = answer.split_once("\r\n\r\n").unwrap().1;
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{path}: {e}: {answer}"))
}

fn stop(mut child: Child) {
    let _ = Command::new("kill").arg(child.id().to_string()).status();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "doze-aws did not stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The summaries of the calls to `action`, oldest first (a call with none is `null`).
fn said(calls: &[Value], action: &str) -> Vec<Value> {
    calls
        .iter()
        .filter(|c| c["action"] == action && c["via"].is_null())
        .map(|c| c["summary"].clone())
        .collect()
}

/// SQS: a send says how big the body was; a receive how many came back; a batch how many were taken.
fn queues(e: &str) {
    json(e, SQS, "CreateQueue", r#"{"QueueName":"orders"}"#);
    json(e, SQS, "CreateQueue", r#"{"QueueName":"mirror"}"#);
    let url = format!("{e}/000000000000/orders");
    json(
        e,
        SQS,
        "SendMessage",
        &format!(r#"{{"QueueUrl":"{url}","MessageBody":"hello"}}"#),
    );
    json(
        e,
        SQS,
        "SendMessageBatch",
        &format!(
            r#"{{"QueueUrl":"{url}","Entries":[{{"Id":"a","MessageBody":"x"}},{{"Id":"b","MessageBody":"y"}}]}}"#
        ),
    );
    json(
        e,
        SQS,
        "ReceiveMessage",
        &format!(r#"{{"QueueUrl":"{url}","MaxNumberOfMessages":10}}"#),
    );
    json(
        e,
        SQS,
        "ReceiveMessage",
        &format!(r#"{{"QueueUrl":"{url}","MaxNumberOfMessages":10}}"#),
    );
    // A refused call has no summary.
    let refused = request(
        e,
        &format!(
            "POST / HTTP/1.1\r\nX-Amz-Target: AmazonSQS.ReceiveMessage\r\nContent-Type: application/x-amz-json-1.0\r\n{}",
            signed("sqs")
        ),
        r#"{"QueueUrl":"http://nowhere/000000000000/none"}"#,
    );
    assert!(!refused.starts_with("HTTP/1.1 200"), "{refused}");
}

/// SNS: a publish says how many deliveries the bus made for it; a subscribe whether it needed a confirmation.
fn topics(e: &str) {
    query(
        e,
        "sns",
        "Action=CreateTopic&Name=placed&Version=2010-03-31",
    );
    let topic = "arn:aws:sns:us-east-1:000000000000:placed";
    for queue in ["orders", "mirror"] {
        let arn = format!("arn:aws:sqs:us-east-1:000000000000:{queue}");
        query(
            e,
            "sns",
            &format!(
                "Action=Subscribe&TopicArn={topic}&Protocol=sqs&Endpoint={arn}&Version=2010-03-31"
            ),
        );
    }
    query(
        e,
        "sns",
        &format!(
            "Action=Subscribe&TopicArn={topic}&Protocol=email&Endpoint=a@b.test&Version=2010-03-31"
        ),
    );
    query(
        e,
        "sns",
        &format!("Action=Publish&TopicArn={topic}&Message=hi&Version=2010-03-31"),
    );
    query(
        e,
        "sns",
        &format!("Action=ListSubscriptionsByTopic&TopicArn={topic}&Version=2010-03-31"),
    );
}

/// S3: a path names the object; a listing counts keys; an object has its size.
fn objects(e: &str) {
    s3(e, "PUT", "/pics", "");
    s3(e, "PUT", "/pics/a/one.txt", "12345");
    s3(e, "PUT", "/pics/a/two.txt", "1234567890123456789012345");
    s3(e, "GET", "/pics?list-type=2", "");
    s3(e, "GET", "/pics/a/one.txt", "");
    s3(e, "GET", "/", "");
    s3(e, "HEAD", "/pics/a/one.txt", "");
}

/// DynamoDB.
fn tables(e: &str) {
    json(
        e,
        DDB,
        "CreateTable",
        r#"{"TableName":"items","AttributeDefinitions":[{"AttributeName":"id","AttributeType":"S"}],"KeySchema":[{"AttributeName":"id","KeyType":"HASH"}],"BillingMode":"PAY_PER_REQUEST"}"#,
    );
    json(
        e,
        DDB,
        "PutItem",
        r#"{"TableName":"items","Item":{"id":{"S":"1"}}}"#,
    );
    json(e, DDB, "Scan", r#"{"TableName":"items"}"#);
    json(
        e,
        DDB,
        "GetItem",
        r#"{"TableName":"items","Key":{"id":{"S":"1"}}}"#,
    );
    json(
        e,
        DDB,
        "GetItem",
        r#"{"TableName":"items","Key":{"id":{"S":"2"}}}"#,
    );
    json(
        e,
        DDB,
        "Query",
        r#"{"TableName":"items","KeyConditionExpression":"id = :i","ExpressionAttributeValues":{":i":{"S":"1"}}}"#,
    );
}

/// Kinesis, EventBridge and Logs.
fn streams(e: &str) {
    json(
        e,
        KINESIS,
        "CreateStream",
        r#"{"StreamName":"k","ShardCount":1}"#,
    );
    json(
        e,
        KINESIS,
        "PutRecord",
        r#"{"StreamName":"k","Data":"aGk=","PartitionKey":"p"}"#,
    );
    json(
        e,
        KINESIS,
        "PutRecords",
        r#"{"StreamName":"k","Records":[{"Data":"aGk=","PartitionKey":"p"},{"Data":"aGk=","PartitionKey":"q"}]}"#,
    );
    json(
        e,
        EVENTS,
        "PutEvents",
        r#"{"Entries":[{"Source":"a","DetailType":"b","Detail":"{}"},{"Source":"a","DetailType":"c","Detail":"{}"}]}"#,
    );
    json(e, LOGS, "CreateLogGroup", r#"{"logGroupName":"g"}"#);
    json(
        e,
        LOGS,
        "CreateLogStream",
        r#"{"logGroupName":"g","logStreamName":"s"}"#,
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    json(
        e,
        LOGS,
        "PutLogEvents",
        &format!(
            r#"{{"logGroupName":"g","logStreamName":"s","logEvents":[{{"timestamp":{now},"message":"a"}},{{"timestamp":{now},"message":"b"}},{{"timestamp":{now},"message":"c"}}]}}"#
        ),
    );
    json(
        e,
        LOGS,
        "GetLogEvents",
        r#"{"logGroupName":"g","logStreamName":"s","startFromHead":true}"#,
    );
}

/// STS and IAM: a session is named by what it was asked for; an attach by who it was for.
fn identities(e: &str) {
    let trust = "%7B%22Version%22%3A%222012-10-17%22%2C%22Statement%22%3A%5B%7B%22Effect%22%3A%22Allow%22%2C%22Principal%22%3A%7B%22AWS%22%3A%22*%22%7D%2C%22Action%22%3A%22sts%3AAssumeRole%22%7D%5D%7D";
    query(
        e,
        "iam",
        &format!(
            "Action=CreateRole&RoleName=report-job&AssumeRolePolicyDocument={trust}&Version=2010-05-08"
        ),
    );
    query(
        e,
        "iam",
        "Action=AttachRolePolicy&RoleName=report-job&PolicyArn=arn%3Aaws%3Aiam%3A%3Aaws%3Apolicy%2FReadOnlyAccess&Version=2010-05-08",
    );
    query(
        e,
        "sts",
        "Action=AssumeRole&RoleArn=arn%3Aaws%3Aiam%3A%3A000000000000%3Arole%2Freport-job&RoleSessionName=nightly&Version=2011-06-15",
    );
}

/// CloudWatch: metrics put, series read.
fn metrics(e: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    json(
        e,
        CLOUDWATCH,
        "PutMetricData",
        r#"{"Namespace":"Larder","MetricData":[{"MetricName":"a","Value":1},{"MetricName":"b","Value":2}]}"#,
    );
    json(
        e,
        CLOUDWATCH,
        "GetMetricData",
        &format!(
            r#"{{"MetricDataQueries":[{{"Id":"q1","MetricStat":{{"Metric":{{"Namespace":"Larder","MetricName":"a"}},"Period":60,"Stat":"Sum"}}}}],"StartTime":{},"EndTime":{}}}"#,
            now - 3600,
            now + 3600
        ),
    );
}

#[test]
fn a_record_says_what_each_call_did_and_only_what_it_can_know() {
    let dir = tempfile::tempdir().unwrap();
    let (child, endpoint, console) = start(dir.path());
    let e = endpoint.as_str();
    queues(e);
    topics(e);
    objects(e);
    tables(e);
    streams(e);
    identities(e);
    metrics(e);

    let page = api(&console, "/api/v1/calls");
    let calls = page["calls"].as_array().unwrap();
    let want = |action: &str, words: &[&str]| {
        let got: Vec<String> = said(calls, action)
            .iter()
            .map(|s| s.as_str().unwrap_or("none").to_owned())
            .collect();
        assert_eq!(got, words, "{action}");
    };
    want("CreateQueue", &["none", "none"]);
    want("SendMessage", &["sent 5 B"]);
    want("SendMessageBatch", &["sent 2 messages"]);
    want("ReceiveMessage", &["3 messages", "no messages", "none"]);
    want("Publish", &["delivered to 2"]);
    want(
        "Subscribe",
        &[
            "confirmed at once",
            "confirmed at once",
            "pending confirmation",
        ],
    );
    want("ListSubscriptionsByTopic", &["3 subscriptions"]);
    want("PutObject", &["5 B", "25 B"]);
    want("ListObjectsV2", &["2 keys"]);
    want("GetObject", &["5 B"]);
    want("ListBuckets", &["1 bucket"]);
    want("Scan", &["1 item"]);
    want("GetItem", &["1 item", "no items"]);
    want("PutRecord", &["1 record"]);
    want("PutRecords", &["2 records"]);
    want("HeadObject", &["5 B"]);
    want("Query", &["1 item"]);
    want("PutMetricData", &["2 metrics"]);
    want("GetMetricData", &["1 series"]);
    want("PutEvents", &["2 events"]);
    want("PutLogEvents", &["3 events"]);
    want("GetLogEvents", &["3 events"]);
    want("AttachRolePolicy", &["to report-job"]);
    want("AssumeRole", &["session nightly"]);
    stop(child);
}
