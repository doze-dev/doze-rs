//! Groups, streams, retention, tags, filters and documents.

use serde_json::json;

use super::World;

#[test]
fn a_group_and_a_stream_are_made_described_and_deleted() {
    let mut w = World::new();
    w.ok(
        "CreateLogGroup",
        &json!({"logGroupName": "/app/web", "tags": {"env": "dev"}}),
    );
    // The members the AWS recording has (test_group_and_stream_lifecycle::describe).
    let d = w.ok("DescribeLogGroups", &json!({"logGroupNamePrefix": "/app"}));
    let g = &d["logGroups"][0];
    assert_eq!(g["logGroupName"], "/app/web");
    assert_eq!(
        g["arn"],
        "arn:aws:logs:us-east-1:000000000000:log-group:/app/web:*"
    );
    assert_eq!(
        g["logGroupArn"],
        "arn:aws:logs:us-east-1:000000000000:log-group:/app/web"
    );
    assert_eq!(g["logGroupClass"], "STANDARD");
    assert_eq!(g["metricFilterCount"], 0);
    assert_eq!(g["deletionProtectionEnabled"], false);
    assert_eq!(g["bearerTokenAuthenticationEnabled"], false);
    assert!(g.get("retentionInDays").is_none());
    w.ok(
        "PutRetentionPolicy",
        &json!({"logGroupName": "/app/web", "retentionInDays": 7}),
    );
    let d = w.ok("DescribeLogGroups", &json!({}));
    assert_eq!(d["logGroups"][0]["retentionInDays"], 7);
    w.ok(
        "DeleteRetentionPolicy",
        &json!({"logGroupName": "/app/web"}),
    );
    assert_eq!(
        w.ok("ListTagsLogGroup", &json!({"logGroupName": "/app/web"}))["tags"],
        json!({"env": "dev"})
    );

    w.stream("/app/web", "app");
    let s = w.ok("DescribeLogStreams", &json!({"logGroupName": "/app/web"}));
    assert_eq!(s["logStreams"][0]["logStreamName"], "app");
    assert_eq!(
        s["logStreams"][0]["arn"],
        "arn:aws:logs:us-east-1:000000000000:log-group:/app/web:log-stream:app"
    );
    assert!(s["logStreams"][0].get("lastEventTimestamp").is_none());
    w.ok(
        "DeleteLogStream",
        &json!({"logGroupName": "/app/web", "logStreamName": "app"}),
    );
    w.ok("DeleteLogGroup", &json!({"logGroupName": "/app/web"}));
    assert_eq!(
        w.ok("DescribeLogGroups", &json!({}))["logGroups"],
        json!([])
    );
}

#[test]
fn the_refusals_are_worded_as_aws_worded_them() {
    let mut w = World::new();
    let (code, msg) = w.err(
        "CreateLogStream",
        &json!({"logGroupName": "/absent", "logStreamName": "a"}),
    );
    assert_eq!(
        (code.as_str(), msg.as_str()),
        (
            "ResourceNotFoundException",
            "The specified log group does not exist."
        )
    );
    w.group("/g");
    let (code, msg) = w.err("CreateLogGroup", &json!({"logGroupName": "/g"}));
    assert_eq!(
        (code.as_str(), msg.as_str()),
        (
            "ResourceAlreadyExistsException",
            "The specified log group already exists"
        )
    );
    let (code, msg) = w.err(
        "PutRetentionPolicy",
        &json!({"logGroupName": "/g", "retentionInDays": 2}),
    );
    assert_eq!(code, "InvalidParameterException");
    assert_eq!(
        msg,
        "Invalid retention value. Valid values are: [1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365, 400, 545, 731, 1096, 1827, 2192, 2557, 2922, 3288, 3653]"
    );
    w.stream("/g", "app");
    let (code, msg) = w.err(
        "CreateLogStream",
        &json!({"logGroupName": "/g", "logStreamName": "app"}),
    );
    assert_eq!(
        (code.as_str(), msg.as_str()),
        (
            "ResourceAlreadyExistsException",
            "The specified log stream already exists"
        )
    );
    let (code, msg) = w.err(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "absent"}),
    );
    assert_eq!(
        (code.as_str(), msg.as_str()),
        (
            "ResourceNotFoundException",
            "The specified log stream does not exist."
        )
    );
    let (code, msg) = w.err(
        "FilterLogEvents",
        &json!({"logGroupName": "/g", "filterPattern": "{ $.code = }"}),
    );
    assert_eq!(
        (code.as_str(), msg.as_str()),
        ("InvalidParameterException", "Invalid filter pattern")
    );
    let (code, _) = w.err(
        "FilterLogEvents",
        &json!({"logGroupName": "/g", "filterPattern": "%x%"}),
    );
    assert_eq!(code, "NotImplemented");
}

#[test]
fn tags_work_by_group_name_and_by_arn() {
    let mut w = World::new();
    w.group("/g");
    w.ok(
        "TagLogGroup",
        &json!({"logGroupName": "/g", "tags": {"a": "1", "b": "2"}}),
    );
    let arn = "arn:aws:logs:us-east-1:000000000000:log-group:/g";
    w.ok(
        "TagResource",
        &json!({"resourceArn": arn, "tags": {"c": "3"}}),
    );
    w.ok(
        "UntagLogGroup",
        &json!({"logGroupName": "/g", "tags": ["a"]}),
    );
    w.ok(
        "UntagResource",
        &json!({"resourceArn": arn, "tagKeys": ["b"]}),
    );
    assert_eq!(
        w.ok("ListTagsForResource", &json!({"resourceArn": arn}))["tags"],
        json!({"c": "3"})
    );
}

#[test]
fn metric_and_subscription_filters_are_stored_and_counted() {
    let mut w = World::new();
    w.group("/g");
    w.ok(
        "PutMetricFilter",
        &json!({"logGroupName": "/g", "filterName": "errors", "filterPattern": "ERROR",
            "metricTransformations": [{"metricName": "Errors", "metricNamespace": "App", "metricValue": "1", "defaultValue": 0.0}]}),
    );
    let d = w.ok("DescribeMetricFilters", &json!({"logGroupName": "/g"}));
    assert_eq!(d["metricFilters"][0]["filterName"], "errors");
    assert_eq!(
        d["metricFilters"][0]["metricTransformations"][0]["metricNamespace"],
        "App"
    );
    assert_eq!(
        w.ok("DescribeLogGroups", &json!({}))["logGroups"][0]["metricFilterCount"],
        1
    );
    w.ok(
        "DeleteMetricFilter",
        &json!({"logGroupName": "/g", "filterName": "errors"}),
    );
    let (code, _) = w.err(
        "DeleteMetricFilter",
        &json!({"logGroupName": "/g", "filterName": "errors"}),
    );
    assert_eq!(code, "ResourceNotFoundException");

    for n in 0..2 {
        w.ok(
            "PutSubscriptionFilter",
            &json!({"logGroupName": "/g", "filterName": format!("s{n}"), "filterPattern": "",
                "destinationArn": "arn:aws:lambda:us-east-1:000000000000:function:f"}),
        );
    }
    let (code, _) = w.err(
        "PutSubscriptionFilter",
        &json!({"logGroupName": "/g", "filterName": "s2", "filterPattern": "", "destinationArn": "arn:aws:lambda:us-east-1:000000000000:function:f"}),
    );
    assert_eq!(code, "LimitExceededException");
    let d = w.ok(
        "DescribeSubscriptionFilters",
        &json!({"logGroupName": "/g"}),
    );
    assert_eq!(d["subscriptionFilters"].as_array().unwrap().len(), 2);
    assert_eq!(d["subscriptionFilters"][0]["distribution"], "ByLogStream");
}

#[test]
fn test_metric_filter_runs_a_pattern_over_messages() {
    let mut w = World::new();
    let out = w.ok(
        "TestMetricFilter",
        &json!({"filterPattern": "[ip, status = 4*]", "logEventMessages": ["1.2.3.4 404", "1.2.3.4 200", "bad"]}),
    );
    let m = out["matches"].as_array().unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0]["eventNumber"], 1);
    assert_eq!(m[0]["extractedValues"]["$status"], "404");
}

#[test]
fn stored_documents_come_back_as_sent() {
    let mut w = World::new();
    let made = w.ok(
        "PutQueryDefinition",
        &json!({"name": "errors", "queryString": "fields @message | limit 5", "logGroupNames": ["/g"]}),
    );
    let id = made["queryDefinitionId"].as_str().unwrap().to_owned();
    let d = w.ok(
        "DescribeQueryDefinitions",
        &json!({"queryDefinitionNamePrefix": "err"}),
    );
    assert_eq!(d["queryDefinitions"][0]["queryDefinitionId"], id);
    assert_eq!(
        d["queryDefinitions"][0]["queryString"],
        "fields @message | limit 5"
    );
    assert_eq!(
        w.ok("DeleteQueryDefinition", &json!({"queryDefinitionId": id}))["success"],
        true
    );

    w.ok(
        "PutResourcePolicy",
        &json!({"policyName": "p", "policyDocument": "{}"}),
    );
    let d = w.ok("DescribeResourcePolicies", &json!({}));
    assert_eq!(d["resourcePolicies"][0]["policyName"], "p");
    w.ok("DeleteResourcePolicy", &json!({"policyName": "p"}));
    assert_eq!(
        w.ok("DescribeResourcePolicies", &json!({}))["resourcePolicies"],
        json!([])
    );

    w.group("/g");
    w.ok(
        "PutDataProtectionPolicy",
        &json!({"logGroupIdentifier": "/g", "policyDocument": "{\"Name\":\"x\"}"}),
    );
    assert_eq!(
        w.ok(
            "GetDataProtectionPolicy",
            &json!({"logGroupIdentifier": "/g"})
        )["policyDocument"],
        "{\"Name\":\"x\"}"
    );
}
