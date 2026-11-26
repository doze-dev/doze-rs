//! What the record says each call did, through the real binary: the services answer, the gateway reads the answer, and the
//! console's `summary` is a few true words (`2 messages`, `sent 5 B`, `delivered to 2`). A call that failed has none, and an
//! operation the table does not know has none.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod summaries_support;

use summaries_support::{api, ok, query, request, said, signed, start, stop};

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

fn s3(endpoint: &str, method: &str, path: &str, body: &str) -> String {
    let head = format!("{method} {path} HTTP/1.1\r\n{}", signed("s3"));
    ok(&request(endpoint, &head, body))
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
    iam_lists(e);
    query(
        e,
        "sts",
        "Action=AssumeRole&RoleArn=arn%3Aaws%3Aiam%3A%3A000000000000%3Arole%2Freport-job&RoleSessionName=nightly&Version=2011-06-15",
    );
}

/// IAM lists say how many they hold, and a policy version read says which one: a user in one group with a policy attached
/// and another written in, an access key, and no device.
fn iam_lists(e: &str) {
    let doc = "%7B%22Version%22%3A%222012-10-17%22%2C%22Statement%22%3A%5B%7B%22Effect%22%3A%22Allow%22%2C%22Action%22%3A%22s3%3AGetObject%22%2C%22Resource%22%3A%22*%22%7D%5D%7D";
    let arn = "arn%3Aaws%3Aiam%3A%3A000000000000%3Apolicy%2Freaders";
    let iam = |form: &str| query(e, "iam", &format!("{form}&Version=2010-05-08"));
    iam("Action=CreateUser&UserName=ann");
    iam("Action=CreateGroup&GroupName=cooks");
    iam("Action=AddUserToGroup&GroupName=cooks&UserName=ann");
    iam(&format!(
        "Action=CreatePolicy&PolicyName=readers&PolicyDocument={doc}"
    ));
    iam(&format!(
        "Action=AttachUserPolicy&UserName=ann&PolicyArn={arn}"
    ));
    iam(&format!(
        "Action=AttachGroupPolicy&GroupName=cooks&PolicyArn={arn}"
    ));
    iam(&format!(
        "Action=PutUserPolicy&UserName=ann&PolicyName=inline&PolicyDocument={doc}"
    ));
    iam(&format!(
        "Action=PutGroupPolicy&GroupName=cooks&PolicyName=inline&PolicyDocument={doc}"
    ));
    iam("Action=CreateAccessKey&UserName=ann");
    for form in [
        "Action=ListGroupsForUser&UserName=ann".to_owned(),
        "Action=ListAttachedUserPolicies&UserName=ann".to_owned(),
        "Action=ListAttachedGroupPolicies&GroupName=cooks".to_owned(),
        "Action=ListAttachedRolePolicies&RoleName=report-job".to_owned(),
        "Action=ListUserPolicies&UserName=ann".to_owned(),
        "Action=ListGroupPolicies&GroupName=cooks".to_owned(),
        "Action=ListRolePolicies&RoleName=report-job".to_owned(),
        "Action=ListAccessKeys&UserName=ann".to_owned(),
        "Action=ListMFADevices&UserName=ann".to_owned(),
        format!("Action=ListEntitiesForPolicy&PolicyArn={arn}"),
        format!("Action=GetPolicyVersion&PolicyArn={arn}&VersionId=v1"),
        format!("Action=ListPolicyVersions&PolicyArn={arn}"),
    ] {
        iam(&form);
    }
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
    want("ListGroupsForUser", &["1 group"]);
    want("ListAttachedUserPolicies", &["1 policy"]);
    want("ListAttachedGroupPolicies", &["1 policy"]);
    want("ListAttachedRolePolicies", &["1 policy"]);
    want("ListUserPolicies", &["1 policy"]);
    want("ListGroupPolicies", &["1 policy"]);
    want("ListRolePolicies", &["no policies"]);
    want("ListAccessKeys", &["1 access key"]);
    want("ListMFADevices", &["no MFA devices"]);
    want("ListEntitiesForPolicy", &["2 entities"]);
    want("GetPolicyVersion", &["v1"]);
    want("ListPolicyVersions", &["1 version"]);
    want("AssumeRole", &["session nightly"]);
    stop(child);
}
