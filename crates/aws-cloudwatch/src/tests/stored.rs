//! Tier B, stored and not active: metric streams, Contributor Insights rules and alarm mute rules round-trip through their Get, List
//! and Describe operations, keep AWS's state values, and take tags by ARN.

use serde_json::json;

use super::{NOW as START, World};

#[test]
fn a_metric_stream_is_stored_listed_stopped_and_deleted() {
    let mut w = World::new();
    let made = w.ok(
        "PutMetricStream",
        &json!({"Name": "all", "FirehoseArn": "arn:aws:firehose:us-east-1:000000000000:deliverystream/d",
            "RoleArn": "arn:aws:iam::000000000000:role/r", "OutputFormat": "json",
            "IncludeFilters": [{"Namespace": "AWS/Lambda"}], "Tags": [{"Key": "team", "Value": "a"}]}),
    );
    let arn = made["Arn"].as_str().unwrap().to_owned();
    assert_eq!(
        arn,
        "arn:aws:cloudwatch:us-east-1:000000000000:metric-stream/all"
    );
    let got = w.ok("GetMetricStream", &json!({"Name": "all"}));
    assert_eq!(got["State"], "running");
    assert_eq!(got["OutputFormat"], "json");
    assert_eq!(got["IncludeFilters"], json!([{"Namespace": "AWS/Lambda"}]));
    assert!(got["CreationDate"].is_number() && got.get("_created").is_none());
    w.ok("StopMetricStreams", &json!({"Names": ["all"]}));
    let listed = w.ok("ListMetricStreams", &json!({}));
    assert_eq!(listed["Entries"][0]["State"], "stopped");
    let tags = w.ok("ListTagsForResource", &json!({"ResourceARN": arn}));
    assert_eq!(tags["Tags"], json!([{"Key": "team", "Value": "a"}]));
    let (code, _) = w.err(
        "PutMetricStream",
        &json!({"Name": "both", "FirehoseArn": "f", "RoleArn": "r", "OutputFormat": "json",
            "IncludeFilters": [{"Namespace": "a"}], "ExcludeFilters": [{"Namespace": "b"}]}),
    );
    assert_eq!(code, "InvalidParameterCombinationException");
    w.ok("DeleteMetricStream", &json!({"Name": "all"}));
    let (code, _) = w.err("GetMetricStream", &json!({"Name": "all"}));
    assert_eq!(code, "ResourceNotFoundException");
}

#[test]
fn an_insight_rule_is_stored_and_reports_nothing_counted() {
    let mut w = World::new();
    let definition =
        json!({"Schema": {"Name": "CloudWatchLogRule", "Version": 1}, "LogGroupNames": ["/app"],
        "LogFormat": "JSON", "Contribution": {"Keys": ["$.ip"]}, "AggregateOn": "Count"})
        .to_string();
    w.ok(
        "PutInsightRule",
        &json!({"RuleName": "ips", "RuleDefinition": definition}),
    );
    let described = w.ok("DescribeInsightRules", &json!({}));
    let rule = &described["InsightRules"][0];
    assert_eq!(
        (rule["Name"].as_str(), rule["State"].as_str()),
        (Some("ips"), Some("ENABLED"))
    );
    assert_eq!(rule["Definition"], definition);
    let off = w.ok(
        "DisableInsightRules",
        &json!({"RuleNames": ["ips", "ghost"]}),
    );
    assert_eq!(off["Failures"][0]["FailureResource"], "ghost");
    assert_eq!(
        w.ok("DescribeInsightRules", &json!({}))["InsightRules"][0]["State"],
        "DISABLED"
    );
    let report = w.ok(
        "GetInsightRuleReport",
        &json!({"RuleName": "ips", "StartTime": 0, "EndTime": 60, "Period": 60}),
    );
    assert_eq!(report["KeyLabels"], json!(["$.ip"]));
    assert_eq!(report["Contributors"], json!([]));
    let (code, _) = w.err(
        "PutInsightRule",
        &json!({"RuleName": "bad", "RuleDefinition": "not json"}),
    );
    assert_eq!(code, "InvalidParameterValueException");
    let gone = w.ok("DeleteInsightRules", &json!({"RuleNames": ["ips"]}));
    assert_eq!(gone["Failures"], json!([]));
}

#[test]
fn a_mute_rule_is_stored_with_its_status_from_its_dates() {
    let mut w = World::new();
    let later = (START / 1000) + 3600;
    w.ok(
        "PutAlarmMuteRule",
        &json!({"Name": "nightly", "Rule": {"Schedule": {"Expression": "cron(0 2 * * *)", "Duration": "PT4H"}},
            "MuteTargets": {"AlarmNames": ["high-cpu"]}, "StartDate": later}),
    );
    let got = w.ok("GetAlarmMuteRule", &json!({"AlarmMuteRuleName": "nightly"}));
    assert_eq!(got["Status"], "SCHEDULED");
    assert_eq!(got["MuteType"], "RECURRING");
    assert_eq!(got["StartDate"].as_f64(), Some(later as f64));
    assert_eq!(
        got["AlarmMuteRuleArn"],
        "arn:aws:cloudwatch:us-east-1:000000000000:alarm-mute-rule:nightly"
    );
    let listed = w.ok(
        "ListAlarmMuteRules",
        &json!({"AlarmName": "high-cpu", "Statuses": ["SCHEDULED"]}),
    );
    assert_eq!(
        listed["AlarmMuteRuleSummaries"].as_array().unwrap().len(),
        1
    );
    let none = w.ok("ListAlarmMuteRules", &json!({"AlarmName": "other"}));
    assert_eq!(none["AlarmMuteRuleSummaries"], json!([]));
    w.clock.set(START + 3_600_001);
    assert_eq!(
        w.ok("GetAlarmMuteRule", &json!({"AlarmMuteRuleName": "nightly"}))["Status"],
        "ACTIVE"
    );
    w.ok(
        "DeleteAlarmMuteRule",
        &json!({"AlarmMuteRuleName": "nightly"}),
    );
    let (code, _) = w.err("GetAlarmMuteRule", &json!({"AlarmMuteRuleName": "nightly"}));
    assert_eq!(code, "ResourceNotFoundException");
}
