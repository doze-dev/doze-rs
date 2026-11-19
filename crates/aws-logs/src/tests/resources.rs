//! Destinations, transformers, field index policies, export tasks and scheduled queries.

use serde_json::json;

use super::{NOW, World};

const STREAM: &str = "arn:aws:kinesis:us-east-1:000000000000:stream/s";

#[test]
fn a_destination_is_kept_with_its_policy_and_tags_and_a_subscription_resolves_to_its_stream() {
    let mut w = World::new();
    let made = w.ok(
        "PutDestination",
        &json!({"destinationName": "d", "targetArn": STREAM, "roleArn": "arn:aws:iam::000000000000:role/r", "tags": {"a": "1"}}),
    );
    let arn = made["destination"]["arn"].as_str().unwrap().to_owned();
    assert_eq!(arn, "arn:aws:logs:us-east-1:000000000000:destination:d");
    assert!(made["destination"].get("tags").is_none());
    w.ok("PutDestinationPolicy", &json!({"destinationName": "d", "accessPolicy": r#"{"Version":"2012-10-17","Statement":[]}"#}));
    let listed = w.ok(
        "DescribeDestinations",
        &json!({"DestinationNamePrefix": "d"}),
    );
    assert_eq!(
        listed["destinations"][0]["accessPolicy"],
        r#"{"Version":"2012-10-17","Statement":[]}"#
    );
    let tags = w.ok("ListTagsForResource", &json!({"resourceArn": arn}));
    assert_eq!(tags["tags"], json!({"a": "1"}));
    assert_eq!(w.core.target_of(&arn), STREAM);
    let (code, _) = w.err("PutDestination", &json!({"destinationName": "x", "targetArn": "arn:aws:sqs:us-east-1:000000000000:q", "roleArn": "r"}));
    assert_eq!(code, "InvalidParameterException");
    let (code, _) = w.err(
        "PutDestinationPolicy",
        &json!({"destinationName": "absent", "accessPolicy": "{}"}),
    );
    assert_eq!(code, "InvalidParameterException");
    w.ok("DeleteDestination", &json!({"destinationName": "d"}));
    let (code, _) = w.err("DeleteDestination", &json!({"destinationName": "d"}));
    assert_eq!(code, "ResourceNotFoundException");
}

#[test]
fn a_transformer_is_kept_tested_and_removed() {
    let mut w = World::new();
    w.group("/t");
    let config =
        json!([{"parseJSON": {}}, {"addKeys": {"entries": [{"key": "env", "value": "dev"}]}}]);
    w.ok(
        "PutTransformer",
        &json!({"logGroupIdentifier": "/t", "transformerConfig": config}),
    );
    let got = w.ok("GetTransformer", &json!({"logGroupIdentifier": "/t"}));
    assert_eq!(got["transformerConfig"], config);
    assert_eq!(
        got["logGroupIdentifier"],
        "arn:aws:logs:us-east-1:000000000000:log-group:/t"
    );
    let tested = w.ok(
        "TestTransformer",
        &json!({"transformerConfig": config, "logEventMessages": [r#"{"a":1}"#]}),
    );
    let out: serde_json::Value = serde_json::from_str(
        tested["transformedLogs"][0]["transformedEventMessage"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(out, json!({"a": 1, "env": "dev"}));
    assert_eq!(tested["transformedLogs"][0]["eventNumber"], 1);
    w.ok("DeleteTransformer", &json!({"logGroupIdentifier": "/t"}));
    // Recorded 2026-11-19: an empty config, not a refusal.
    assert_eq!(
        w.ok("GetTransformer", &json!({"logGroupIdentifier": "/t"})),
        json!({"transformerConfig": []})
    );
    let (code, _) = w.err("PutTransformer", &json!({"logGroupIdentifier": "/t", "transformerConfig": [{"parseToOCSF": {"eventSource": "VPCFlow", "ocsfVersion": "V1.1"}}]}));
    assert_eq!(code, "InvalidParameterException");
    assert_eq!(
        w.err(
            "PutTransformer",
            &json!({"logGroupIdentifier": "/absent", "transformerConfig": config})
        )
        .0,
        "ResourceNotFoundException"
    );
}

#[test]
fn an_index_policy_is_checked_kept_and_its_indexes_described() {
    let mut w = World::new();
    w.group("/i");
    w.stream("/i", "s");
    w.put("/i", "s", NOW, &[r#"{"requestId":"r1"}"#, "no fields"]);
    let doc = r#"{"Fields":["requestId"],"FieldsV2":{"status":{"type":"FACET"}}}"#;
    let put = w.ok(
        "PutIndexPolicy",
        &json!({"logGroupIdentifier": "/i", "policyDocument": doc}),
    );
    assert_eq!(put["indexPolicy"]["source"], "LOG_GROUP");
    let policies = w.ok(
        "DescribeIndexPolicies",
        &json!({"logGroupIdentifiers": ["/i"]}),
    );
    assert_eq!(policies["indexPolicies"][0]["policyDocument"], doc);
    let indexes = w.ok(
        "DescribeFieldIndexes",
        &json!({"logGroupIdentifiers": ["/i"], "indexCategories": ["CUSTOM"]}),
    );
    // Recorded 2026-11-19: only the fields some event holds are listed (`status` is in none).
    let list = indexes["fieldIndexes"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["fieldIndexName"], "requestId");
    assert_eq!(list[0]["firstEventTime"], NOW);
    for bad in [
        r#"{"Fields":[]}"#,
        r#"{"Fields":["a"],"FieldsV2":{"a":{"type":"FACET"}}}"#,
        "not json",
    ] {
        assert_eq!(
            w.err(
                "PutIndexPolicy",
                &json!({"logGroupIdentifier": "/i", "policyDocument": bad})
            )
            .0,
            "InvalidParameterException",
            "{bad}"
        );
    }
    w.ok("DeleteIndexPolicy", &json!({"logGroupIdentifier": "/i"}));
    assert_eq!(
        w.err("DeleteIndexPolicy", &json!({"logGroupIdentifier": "/i"})),
        (
            "ResourceNotFoundException".into(),
            "Specified resource does not exist!".into()
        )
    );
}

#[test]
fn an_export_task_is_recorded_and_cancelled_while_pending() {
    let mut w = World::new();
    w.group("/e");
    let made = w.ok(
        "CreateExportTask",
        &json!({"logGroupName": "/e", "from": 0, "to": NOW, "destination": "bucket"}),
    );
    let id = made["taskId"].as_str().unwrap().to_owned();
    let tasks = w.ok("DescribeExportTasks", &json!({"taskId": id}));
    assert_eq!(tasks["exportTasks"][0]["status"]["code"], "PENDING");
    assert_eq!(tasks["exportTasks"][0]["destinationPrefix"], "exportedlogs");
    w.ok("CancelExportTask", &json!({"taskId": id}));
    let tasks = w.ok("DescribeExportTasks", &json!({"statusCode": "CANCELLED"}));
    assert_eq!(tasks["exportTasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        w.err("CancelExportTask", &json!({"taskId": id})).0,
        "InvalidOperationException"
    );
    assert_eq!(
        w.err("CancelExportTask", &json!({"taskId": "absent"})).0,
        "ResourceNotFoundException"
    );
    assert_eq!(
        w.err(
            "CreateExportTask",
            &json!({"logGroupName": "/absent", "from": 0, "to": 1, "destination": "b"})
        )
        .0,
        "ResourceNotFoundException"
    );
}

#[test]
fn a_scheduled_query_runs_when_due_and_keeps_its_history() {
    let mut w = World::new();
    w.group("/s");
    w.stream("/s", "x");
    w.put("/s", "x", NOW - 10_000, &["one", "two"]);
    let made = w.ok(
        "CreateScheduledQuery",
        &json!({"name": "every-minute", "queryLanguage": "CWLI", "queryString": "stats count(*) as n",
                "logGroupIdentifiers": ["/s"], "scheduleExpression": "rate(1 minute)", "startTimeOffset": 3600,
                "executionRoleArn": "arn:aws:iam::000000000000:role/r"}),
    );
    assert_eq!(made["state"], "ENABLED");
    let arn = made["scheduledQueryArn"].as_str().unwrap().to_owned();
    assert_eq!(w.core.next_due(), Some(NOW + 60_000));
    assert_eq!(w.core.tick(), Some(NOW + 60_000));
    w.clock.advance(61_000);
    assert_eq!(w.core.tick(), Some(NOW + 120_000));
    let history = w.ok(
        "GetScheduledQueryHistory",
        &json!({"identifier": "every-minute", "startTime": 0, "endTime": NOW + 120_000}),
    );
    let runs = history["triggerHistory"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["executionStatus"], "Complete");
    let id = runs[0]["queryId"].as_str().unwrap();
    let results = w.ok("GetQueryResults", &json!({"queryId": id}));
    assert_eq!(results["results"][0][0]["value"], "2");
    let got = w.ok("GetScheduledQuery", &json!({"identifier": arn}));
    assert_eq!(got["lastExecutionStatus"], "Complete");
    assert_eq!(got["scheduleType"], "CUSTOMER_MANAGED");
    // Recorded 2026-11-19: an end offset not given is shown as 0.
    assert_eq!(got["endTimeOffset"], 0);
    let listed = w.ok("ListScheduledQueries", &json!({"state": "ENABLED"}));
    assert_eq!(listed["scheduledQueries"][0]["name"], "every-minute");
    assert_eq!(w.err("CreateScheduledQuery", &json!({"name": "every-minute", "queryLanguage": "CWLI", "queryString": "fields x", "logGroupIdentifiers": ["/s"], "startTimeOffset": 60, "scheduleExpression": "rate(1 minute)", "executionRoleArn": "r"})).0, "ConflictException");
    // Recorded 2026-11-19: with no group named the query is refused (as one that does not name its own) before the name is looked at.
    let (code, message) = w.err("CreateScheduledQuery", &json!({"name": "every-minute", "queryLanguage": "CWLI", "queryString": "fields x", "startTimeOffset": 60, "scheduleExpression": "rate(1 minute)", "executionRoleArn": "r"}));
    assert_eq!(code, "ValidationException");
    assert!(message.starts_with("Invalid syntax while using query definition snippets: unexpected symbol found fields at line 1 and position 0 (Service: AWSLogs; Status Code: 400; Error Code: MalformedQueryException;"), "{message}");
    assert_eq!(w.err("CreateScheduledQuery", &json!({"name": "bad", "queryLanguage": "CWLI", "queryString": "fields x", "scheduleExpression": "every minute", "executionRoleArn": "r"})).0, "ValidationException");
    w.ok("UpdateScheduledQuery", &json!({"identifier": "every-minute", "queryLanguage": "CWLI", "queryString": "fields x", "logGroupIdentifiers": ["/s"], "scheduleExpression": "rate(5 minutes)", "executionRoleArn": "r", "startTimeOffset": 300, "state": "DISABLED"}));
    assert_eq!(w.core.next_due(), None);
    w.ok("DeleteScheduledQuery", &json!({"identifier": arn}));
    assert_eq!(
        w.err("GetScheduledQuery", &json!({"identifier": "every-minute"})),
        (
            "ResourceNotFoundException".into(),
            "Scheduled query with identifier every-minute not found".into()
        )
    );
}
