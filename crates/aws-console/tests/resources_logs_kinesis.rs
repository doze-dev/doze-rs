//! The Logs and Kinesis pages of their own: what a log group's page reads (its streams, metric filters, subscription filters, class
//! badge) and the operations its pages make, each bound to the group it acts on; a stream's settings, Data Viewer reads and sharing
//! policy, and the delete that takes its consumers with it. Against real services on a real bus.
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
use world::{encode, field, firsts, sub, world};

const BASE: &str = "/api/v1/resources";

fn group(name: &str) -> String {
    format!("{BASE}/logs/groups/{}", encode(name))
}

async fn op(w: &world::World, path: &str, operation: &str, body: &Value) -> (u16, Value) {
    w.send("POST", &format!("{path}/op/{operation}"), body)
        .await
}

#[tokio::test]
async fn a_log_group_is_created_with_its_key_and_protection_and_the_list_says_its_retention_and_filters()
 {
    let w = world("lk-group", Mode::Soft, &[]);
    let (status, made) = w
        .send(
            "POST",
            &format!("{BASE}/logs/groups/op/CreateLogGroup"),
            &json!({"logGroupName": "/larder/api", "logGroupClass": "STANDARD",
                "kmsKeyId": "arn:aws:kms:us-east-1:000000000000:key/abc", "deletionProtectionEnabled": true}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    let one = group("/larder/api");
    let (status, d) = w.get(&one).await;
    assert_eq!(status, 200, "{d}");
    assert_eq!(
        field(&d, "KMS key")["value"],
        "arn:aws:kms:us-east-1:000000000000:key/abc"
    );
    assert_eq!(d["badges"][0]["text"], "Standard");
    assert_eq!(d["verbs"][0]["id"], "search");
    // Protected from a delete until the protection is lifted (the service's own refusal passes through).
    let (status, refused) = w.send("DELETE", &one, &json!({})).await;
    assert_eq!(status, 400, "{refused}");
    // An operation the kind does not list is not made, whatever it names.
    let (status, no) = w
        .send(
            "POST",
            &format!("{BASE}/logs/groups/op/DeleteLogGroup"),
            &json!({"logGroupName": "/larder/api"}),
        )
        .await;
    assert_eq!((status, no["code"].as_str()), (404, Some("NOT_IN_PAGE")));

    // The retention is set and cleared on this group; its list says it in the columns the board names.
    let (status, set) = op(
        &w,
        &one,
        "PutRetentionPolicy",
        &json!({"retentionInDays": 30}),
    )
    .await;
    assert_eq!(status, 200, "{set}");
    let (_, list) = w.get(&format!("{BASE}/logs/groups")).await;
    let titles: Vec<&str> = list["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        ["Class", "Retention (days)", "Metric filters", "Created"]
    );
    assert_eq!(list["rows"][0]["cells"][0], "STANDARD");
    assert_eq!(list["rows"][0]["cells"][1], "30");
    assert_eq!(list["rows"][0]["cells"][2], "0");
    let (status, cleared) = op(&w, &one, "DeleteRetentionPolicy", &json!({})).await;
    assert_eq!(status, 200, "{cleared}");
    let (_, d) = w.get(&one).await;
    assert_eq!(field(&d, "Retention (days)")["value"], "Never expire");
    // A page cannot point a call at another group: the group in the address is the one acted on.
    w.app(
        "logs",
        "CreateLogGroup",
        &json!({"logGroupName": "/larder/other"}),
    )
    .await;
    op(
        &w,
        &one,
        "PutRetentionPolicy",
        &json!({"logGroupName": "/larder/other", "retentionInDays": 7}),
    )
    .await;
    let (_, other) = w.get(&group("/larder/other")).await;
    assert_eq!(field(&other, "Retention (days)")["value"], "Never expire");
    let (_, mine) = w.get(&one).await;
    assert_eq!(field(&mine, "Retention (days)")["value"], "7");
}

#[tokio::test]
async fn a_pattern_is_tested_and_a_metric_filter_made_read_and_deleted_on_its_group() {
    let w = world("lk-filters", Mode::Soft, &[]);
    w.app(
        "logs",
        "CreateLogGroup",
        &json!({"logGroupName": "/larder/api"}),
    )
    .await;
    // TestMetricFilter makes nothing and names no group: the matches say their event numbers.
    let (status, tested) = w
        .send(
            "POST",
            &format!("{BASE}/logs/groups/op/TestMetricFilter"),
            &json!({"filterPattern": "{ $.level = \"error\" }",
                "logEventMessages": ["{\"level\":\"error\",\"msg\":\"a\"}", "{\"level\":\"info\"}", "{\"level\":\"error\",\"msg\":\"c\"}"]}),
        )
        .await;
    assert_eq!(status, 200, "{tested}");
    let numbers: Vec<i64> = tested["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["eventNumber"].as_i64().unwrap())
        .collect();
    assert_eq!(numbers, [1, 3]);
    // A pattern the service does not accept is its refusal, in its words.
    let (status, bad) = w
        .send(
            "POST",
            &format!("{BASE}/logs/groups/op/TestMetricFilter"),
            &json!({"filterPattern": "{ $.level = }", "logEventMessages": ["x"]}),
        )
        .await;
    assert_eq!(status, 400, "{bad}");

    let one = group("/larder/api");
    let (status, put) = op(
        &w,
        &one,
        "PutMetricFilter",
        &json!({"filterName": "api-errors", "filterPattern": "{ $.level = \"error\" }",
            "metricTransformations": [{"metricNamespace": "Larder/Api", "metricName": "ApiErrors", "metricValue": "1", "unit": "Count"}]}),
    )
    .await;
    assert_eq!(status, 200, "{put}");
    let (_, d) = w.get(&one).await;
    assert_eq!(field(&d, "Metric filters")["value"], "1");
    let filters = sub(&d, "Metric filters");
    assert_eq!(filters["rows"][0]["cells"][0], "api-errors");
    assert_eq!(filters["rows"][0]["cells"][2], "ApiErrors");
    assert_eq!(filters["rows"][0]["cells"][3], "Larder/Api");
    assert_eq!(filters["rows"][0]["cells"][5], "Count");
    // The account's filters are read without a group (a new filter's namespace says what the others hold).
    let (status, all) = w
        .send(
            "POST",
            &format!("{BASE}/logs/groups/op/DescribeMetricFilters"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{all}");
    assert_eq!(
        all["metricFilters"][0]["metricTransformations"][0]["metricNamespace"],
        "Larder/Api"
    );
    let (status, gone) = op(
        &w,
        &one,
        "DeleteMetricFilter",
        &json!({"filterName": "api-errors"}),
    )
    .await;
    assert_eq!(status, 200, "{gone}");
    let (_, d) = w.get(&one).await;
    assert!(
        sub(&d, "Metric filters")["rows"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_group_lists_its_streams_newest_first_and_takes_a_subscription_filter() {
    let w = world("lk-streams", Mode::Soft, &[]);
    w.app(
        "logs",
        "CreateLogGroup",
        &json!({"logGroupName": "/larder/api"}),
    )
    .await;
    for s in ["old-stream", "new-stream"] {
        w.app(
            "logs",
            "CreateLogStream",
            &json!({"logGroupName": "/larder/api", "logStreamName": s}),
        )
        .await;
    }
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    for (s, at) in [("old-stream", now - 60_000), ("new-stream", now - 1_000)] {
        w.app(
            "logs",
            "PutLogEvents",
            &json!({"logGroupName": "/larder/api", "logStreamName": s, "logEvents": [{"timestamp": at, "message": "hello"}]}),
        )
        .await;
    }
    let one = group("/larder/api");
    let (_, d) = w.get(&one).await;
    assert_eq!(firsts(sub(&d, "Log streams")), ["new-stream", "old-stream"]);
    // A subscription filter names its destination; the group's page lists it.
    let (status, put) = op(
        &w,
        &one,
        "PutSubscriptionFilter",
        &json!({"filterName": "errors-to-email", "filterPattern": "ERROR",
            "destinationArn": "arn:aws:lambda:us-east-1:000000000000:function:send-email"}),
    )
    .await;
    assert_eq!(status, 200, "{put}");
    let (_, d) = w.get(&one).await;
    let subs = sub(&d, "Subscription filters");
    assert_eq!(subs["rows"][0]["cells"][0], "errors-to-email");
    let (status, gone) = op(
        &w,
        &one,
        "DeleteSubscriptionFilter",
        &json!({"filterName": "errors-to-email"}),
    )
    .await;
    assert_eq!(status, 200, "{gone}");
}

#[tokio::test]
async fn a_stream_is_made_with_its_tags_and_each_setting_is_its_own_call() {
    let w = world("lk-stream", Mode::Soft, &[]);
    let (status, made) = w
        .send(
            "POST",
            &format!("{BASE}/kinesis/streams/op/CreateStream"),
            &json!({"StreamName": "clickstream", "StreamModeDetails": {"StreamMode": "PROVISIONED"}, "ShardCount": 2,
                "Tags": {"stack": "larder", "env": "dev"}}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    let one = format!("{BASE}/kinesis/streams/clickstream");
    let (status, d) = w.get(&one).await;
    assert_eq!(status, 200, "{d}");
    assert_eq!(d["badges"][0]["text"], "ACTIVE");
    assert_eq!(field(&d, "Open shards")["value"], "2");
    assert_eq!(d["tags"], true);
    let (_, tags) = w.get(&format!("{one}/tags")).await;
    assert_eq!(tags["tags"].as_array().unwrap().len(), 2);
    // The list's columns are the board's; the consumers are counted for its sentence.
    let (_, list) = w.get(&format!("{BASE}/kinesis/streams")).await;
    let titles: Vec<&str> = list["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Status", "Capacity mode", "Created", "Consumers"]);

    let (status, r) = op(
        &w,
        &one,
        "UpdateShardCount",
        &json!({"TargetShardCount": 4, "ScalingType": "UNIFORM_SCALING"}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (status, r) = op(
        &w,
        &one,
        "IncreaseStreamRetentionPeriod",
        &json!({"RetentionPeriodHours": 72}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (status, r) = op(
        &w,
        &one,
        "StartStreamEncryption",
        &json!({"EncryptionType": "KMS", "KeyId": "alias/larder-app"}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (status, r) = op(
        &w,
        &one,
        "EnableEnhancedMonitoring",
        &json!({"ShardLevelMetrics": ["IncomingBytes"]}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (_, d) = w.get(&one).await;
    // A list of names arrives as JSON text, which the page reads.
    assert_eq!(
        field(&d, "Shard-level metrics")["value"],
        "[\"IncomingBytes\"]"
    );
    assert_eq!(field(&d, "Open shards")["value"], "4");
    assert_eq!(field(&d, "Retention (hours)")["value"], "72");
    assert_eq!(field(&d, "Encryption")["value"], "KMS");
    let (status, r) = op(
        &w,
        &one,
        "DecreaseStreamRetentionPeriod",
        &json!({"RetentionPeriodHours": 24}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    // A decrease that is an increase is the service's refusal.
    let (status, r) = op(
        &w,
        &one,
        "DecreaseStreamRetentionPeriod",
        &json!({"RetentionPeriodHours": 48}),
    )
    .await;
    assert_eq!(status, 400, "{r}");
    let (status, r) = op(
        &w,
        &one,
        "StopStreamEncryption",
        &json!({"EncryptionType": "KMS", "KeyId": "alias/larder-app"}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (status, r) = op(
        &w,
        &one,
        "UpdateStreamMode",
        &json!({"StreamModeDetails": {"StreamMode": "ON_DEMAND"}}),
    )
    .await;
    assert_eq!(status, 200, "{r}");
    let (_, d) = w.get(&one).await;
    assert_eq!(field(&d, "Capacity mode")["value"], "ON_DEMAND");
    // The encryption edit's keys: KMS's aliases, read through the kind.
    let (status, aliases) = w
        .send(
            "POST",
            &format!("{BASE}/kinesis/streams/op/ListAliases"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{aliases}");
    // A call cannot be pointed at another stream: the one in the address is the one acted on.
    w.app(
        "kinesis",
        "CreateStream",
        &json!({"StreamName": "other", "ShardCount": 1}),
    )
    .await;
    op(
        &w,
        &one,
        "IncreaseStreamRetentionPeriod",
        &json!({"StreamName": "other", "RetentionPeriodHours": 96}),
    )
    .await;
    let (_, other) = w.get(&format!("{BASE}/kinesis/streams/other")).await;
    assert_eq!(field(&other, "Retention (hours)")["value"], "24");
    let (_, mine) = w.get(&one).await;
    assert_eq!(field(&mine, "Retention (hours)")["value"], "96");
}

#[tokio::test]
async fn a_streams_records_are_read_through_an_iterator_and_its_sharing_policy_is_kept() {
    let w = world("lk-viewer", Mode::Soft, &[]);
    w.app(
        "kinesis",
        "CreateStream",
        &json!({"StreamName": "clickstream", "ShardCount": 1}),
    )
    .await;
    for (i, key) in ["s-0a3d", "s-91b7"].iter().enumerate() {
        w.app(
            "kinesis",
            "PutRecord",
            &json!({"StreamName": "clickstream", "PartitionKey": key, "Data": format!("e{i}")}),
        )
        .await;
    }
    let one = format!("{BASE}/kinesis/streams/clickstream");
    let (status, shards) = op(&w, &one, "ListShards", &json!({})).await;
    assert_eq!(status, 200, "{shards}");
    let shard = shards["Shards"][0]["ShardId"].as_str().unwrap().to_owned();
    let (status, it) = op(
        &w,
        &one,
        "GetShardIterator",
        &json!({"ShardId": shard, "ShardIteratorType": "TRIM_HORIZON"}),
    )
    .await;
    assert_eq!(status, 200, "{it}");
    let (status, got) = op(
        &w,
        &one,
        "GetRecords",
        &json!({"ShardIterator": it["ShardIterator"], "Limit": 50}),
    )
    .await;
    assert_eq!(status, 200, "{got}");
    assert_eq!(got["Records"].as_array().unwrap().len(), 2);
    assert!(got["NextShardIterator"].is_string());

    // The sharing policy: none (the service answers an empty one), put, read back, deleted.
    let (status, none) = op(&w, &one, "GetResourcePolicy", &json!({})).await;
    assert_eq!(status, 200, "{none}");
    assert_eq!(none["Policy"], "{}");
    let policy = json!({"Version": "2012-10-17", "Statement": [{"Sid": "read", "Effect": "Allow",
        "Principal": {"AWS": "arn:aws:iam::000000000000:role/report-job"},
        "Action": ["kinesis:GetRecords"], "Resource": "arn:aws:kinesis:us-east-1:000000000000:stream/clickstream"}]});
    let (status, put) = op(
        &w,
        &one,
        "PutResourcePolicy",
        &json!({"Policy": policy.to_string()}),
    )
    .await;
    assert_eq!(status, 200, "{put}");
    let (_, back) = op(&w, &one, "GetResourcePolicy", &json!({})).await;
    assert_eq!(
        serde_json::from_str::<Value>(back["Policy"].as_str().unwrap()).unwrap(),
        policy
    );
    let (status, gone) = op(&w, &one, "DeleteResourcePolicy", &json!({})).await;
    assert_eq!(status, 200, "{gone}");
}

#[tokio::test]
async fn a_stream_with_a_consumer_is_deleted_with_it_and_the_consumers_are_listed_first() {
    let w = world("lk-delete", Mode::Soft, &[]);
    let made = w
        .app(
            "kinesis",
            "CreateStream",
            &json!({"StreamName": "clickstream", "ShardCount": 1}),
        )
        .await;
    drop(made);
    let (_, d) = w.get(&format!("{BASE}/kinesis/streams/clickstream")).await;
    let arn = field(&d, "ARN")["value"].as_str().unwrap().to_owned();
    w.app(
        "kinesis",
        "RegisterStreamConsumer",
        &json!({"StreamARN": arn, "ConsumerName": "search-indexer"}),
    )
    .await;
    let (status, consumers) = w
        .send(
            "POST",
            &format!("{BASE}/kinesis/streams/op/ListStreamConsumers"),
            &json!({"StreamARN": arn}),
        )
        .await;
    assert_eq!(status, 200, "{consumers}");
    assert_eq!(consumers["Consumers"][0]["ConsumerName"], "search-indexer");
    let (_, list) = w.get(&format!("{BASE}/kinesis/streams")).await;
    assert_eq!(list["rows"][0]["cells"][3], "1");
    // Deleting takes the consumer with the stream, as the dialog says.
    w.delete(&format!("{BASE}/kinesis/streams"), "clickstream")
        .await;
}
