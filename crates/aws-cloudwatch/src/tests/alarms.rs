use serde_json::json;

use super::{NOW, World};

#[test]
fn an_alarm_is_shown_as_the_recording_shows_it() {
    // test_alarm_lifecycle::describe, 2026-11-09.
    let mut w = World::new();
    w.alarm(
        "a",
        &json!({"AlarmDescription": "queue is backing up", "Dimensions": [{"Name": "queue", "Value": "orders"}],
            "EvaluationPeriods": 2, "DatapointsToAlarm": 2, "TreatMissingData": "notBreaching", "ActionsEnabled": false}),
    );
    let d = w.ok("DescribeAlarms", &json!({"AlarmNames": ["a"]}));
    assert_eq!(d["CompositeAlarms"], json!([]));
    assert_eq!(d["LogAlarms"], json!([]));
    let a = &d["MetricAlarms"][0];
    assert_eq!(
        a["AlarmArn"],
        "arn:aws:cloudwatch:us-east-1:000000000000:alarm:a"
    );
    assert_eq!(a["ActionsEnabled"], false);
    for list in ["AlarmActions", "OKActions", "InsufficientDataActions"] {
        assert_eq!(a[list], json!([]), "{list}");
    }
    assert_eq!(a["DatapointsToAlarm"], 2);
    assert_eq!(a["Threshold"], 100.0);
    assert_eq!(a["StateValue"], "INSUFFICIENT_DATA");
    assert_eq!(a["StateReason"], "Unchecked: Initial alarm creation");
    assert!(a.get("Unit").is_none() && a.get("EvaluateLowSampleCountPercentile").is_none());
    assert!(a["AlarmConfigurationUpdatedTimestamp"].is_number());
}

#[test]
fn a_forced_state_stands_and_deleting_twice_is_accepted() {
    let mut w = World::new();
    w.alarm("a", &json!({}));
    w.ok(
        "SetAlarmState",
        &json!({"AlarmName": "a", "StateValue": "ALARM", "StateReason": "forced by the scenario"}),
    );
    let d = w.ok("DescribeAlarms", &json!({"AlarmNames": ["a"]}));
    assert_eq!(d["MetricAlarms"][0]["StateValue"], "ALARM");
    assert_eq!(
        d["MetricAlarms"][0]["StateReason"],
        "forced by the scenario"
    );
    let by_prefix = w.ok("DescribeAlarms", &json!({"AlarmNamePrefix": "a"}));
    assert_eq!(by_prefix["MetricAlarms"].as_array().unwrap().len(), 1);
    w.ok("DisableAlarmActions", &json!({"AlarmNames": ["a"]}));
    w.ok("DeleteAlarms", &json!({"AlarmNames": ["a"]}));
    w.ok("DeleteAlarms", &json!({"AlarmNames": ["a"]}));
    assert_eq!(
        w.ok("DescribeAlarms", &json!({"AlarmNames": ["a"]}))["MetricAlarms"],
        json!([])
    );
    // The history outlives the alarm (C2 of the audit decisions: the model says AWS retains it).
    let h = w.ok("DescribeAlarmHistory", &json!({"AlarmName": "a"}));
    assert!(h["AlarmHistoryItems"].as_array().unwrap().len() >= 2);
}

#[test]
fn the_refusals_are_worded_as_aws_worded_them() {
    let mut w = World::new();
    let base = json!({"AlarmName": "a", "Namespace": "T", "MetricName": "m", "Statistic": "Sum", "Period": 60,
        "EvaluationPeriods": 1, "Threshold": 1, "ComparisonOperator": "GreaterThanThreshold"});
    let with = |k: &str, v: serde_json::Value| {
        let mut r = base.clone();
        r[k] = v;
        r
    };
    let (code, msg) = w.err("PutMetricAlarm", &with("DatapointsToAlarm", json!(3)));
    assert_eq!(
        (code.as_str(), msg.as_str()),
        (
            "ValidationException",
            "DatapointsToAlarm must be less than or equal to EvaluationPeriods"
        )
    );
    let (_, msg) = w.err("PutMetricAlarm", &with("ExtendedStatistic", json!("p99")));
    assert_eq!(
        msg,
        "PutMetricAlarm request can not be made with both Statistic and ExtendedStatistic parameters. Only one of them should be present"
    );
    let (_, msg) = w.err("PutMetricAlarm", &with("TreatMissingData", json!("shrug")));
    assert_eq!(
        msg,
        "The value shrug is not supported for TreatMissingData parameter. Supported values are [breaching, notBreaching, ignore, missing]."
    );
    let (code, msg) = w.err(
        "SetAlarmState",
        &json!({"AlarmName": "absent", "StateValue": "OK", "StateReason": "x"}),
    );
    assert_eq!((code.as_str(), msg.as_str()), ("ResourceNotFound", ""));
    // A bad enum is a ValidationError worded from the model; a bad unit is the InvalidParameterValue AWS answered with.
    let e = w
        .run(
            "PutMetricAlarm",
            &with("ComparisonOperator", json!("RoughlyEqualTo")),
        )
        .unwrap_err();
    assert_eq!(e.shape.as_deref(), Some("ValidationException"));
    assert!(e.message.starts_with("1 validation error detected: Value 'RoughlyEqualTo' at 'comparisonOperator' failed to satisfy constraint: Member must satisfy enum value set: ["));
    let e = w.run("PutMetricData", &json!({"Namespace": "T", "MetricData": [{"MetricName": "m", "Value": 1, "Unit": "Furlongs"}]})).unwrap_err();
    assert_eq!(e.shape.as_deref(), Some("InvalidParameterValueException"));
}

#[test]
fn an_alarm_follows_its_data_and_does_not_flip_back_for_want_of_a_poll() {
    let mut w = World::new();
    w.alarm("a", &json!({"TreatMissingData": "notBreaching"}));
    // Nothing is breaching and the alarm treats missing data as fine: its first look is due now.
    assert_eq!(w.core.earliest_due(), Some(NOW));
    w.core.tick();
    assert_eq!(w.state("a"), "OK");
    assert_eq!(
        w.core.earliest_due(),
        None,
        "no data can change an OK alarm with nothing in the window"
    );
    // Data that breaches judges the alarm at once.
    w.put("m", NOW + 10_000, 150.0);
    assert_eq!(w.state("a"), "ALARM");
    // With nothing new, the alarm goes back when the datapoint leaves its one-period window: a deadline, not a poll.
    assert_eq!(w.core.earliest_due(), Some(NOW + 60_000));
    w.clock.advance(59_000);
    w.core.tick();
    assert_eq!(w.state("a"), "ALARM", "not yet");
    w.clock.advance(1_000);
    w.core.tick();
    assert_eq!(w.state("a"), "OK");
    assert_eq!(w.core.earliest_due(), None);
    let h = w.ok("DescribeAlarmHistory", &json!({"AlarmName": "a", "HistoryItemType": "StateUpdate", "ScanBy": "TimestampAscending"}));
    let summaries: Vec<&str> = h["AlarmHistoryItems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["HistorySummary"].as_str().unwrap())
        .collect();
    assert_eq!(
        summaries,
        [
            "Alarm updated from INSUFFICIENT_DATA to OK",
            "Alarm updated from OK to ALARM",
            "Alarm updated from ALARM to OK"
        ]
    );
}

#[test]
fn missing_data_is_judged_by_the_setting() {
    for (treat, want) in [
        ("breaching", "ALARM"),
        ("notBreaching", "OK"),
        ("missing", "INSUFFICIENT_DATA"),
    ] {
        let mut w = World::new();
        w.alarm(
            "a",
            &json!({"TreatMissingData": treat, "EvaluationPeriods": 3, "DatapointsToAlarm": 2}),
        );
        w.clock.advance(180_000);
        w.core.tick();
        assert_eq!(w.state("a"), want, "{treat}");
    }
    // M of N: two breaching datapoints of three alarm; one does not.
    let mut w = World::new();
    w.alarm("a", &json!({"EvaluationPeriods": 3, "DatapointsToAlarm": 2, "TreatMissingData": "notBreaching"}));
    w.put("m", NOW - 60_000, 200.0);
    assert_eq!(w.state("a"), "OK");
    w.put("m", NOW, 200.0);
    assert_eq!(w.state("a"), "ALARM");
}

#[test]
fn a_forced_state_is_kept_until_the_next_period_and_then_judged_again() {
    let mut w = World::new();
    w.alarm("a", &json!({"TreatMissingData": "notBreaching"}));
    w.core.tick();
    w.ok(
        "SetAlarmState",
        &json!({"AlarmName": "a", "StateValue": "ALARM", "StateReason": "test"}),
    );
    assert_eq!(w.core.earliest_due(), Some(NOW + 60_000));
    w.clock.advance(60_000);
    w.core.tick();
    assert_eq!(w.state("a"), "OK");
}

#[test]
fn math_and_composite_alarms_are_stored_and_say_they_are_not_evaluated() {
    let mut w = World::new();
    w.ok(
        "PutMetricAlarm",
        &json!({"AlarmName": "m", "EvaluationPeriods": 1, "Threshold": 1, "ComparisonOperator": "GreaterThanThreshold",
            "Metrics": [{"Id": "e", "Expression": "a + b", "ReturnData": true}]}),
    );
    w.ok(
        "PutCompositeAlarm",
        &json!({"AlarmName": "c", "AlarmRule": "ALARM(m)"}),
    );
    let d = w.ok("DescribeAlarms", &json!({}));
    assert_eq!(d["MetricAlarms"][0]["Metrics"][0]["Expression"], "a + b");
    assert!(
        d["MetricAlarms"][0]["StateReason"]
            .as_str()
            .unwrap()
            .starts_with("Not evaluated locally")
    );
    assert_eq!(d["CompositeAlarms"][0]["AlarmRule"], "ALARM(m)");
    assert_eq!(w.core.earliest_due(), None);
    let only = w.ok("DescribeAlarms", &json!({"AlarmTypes": ["CompositeAlarm"]}));
    assert_eq!(only["MetricAlarms"], json!([]));
}

#[test]
fn actions_are_queued_on_a_change_and_the_history_says_they_were_not_delivered() {
    let mut w = World::new();
    w.alarm(
        "a",
        &json!({"AlarmActions": ["arn:aws:sns:us-east-1:000000000000:t"], "OKActions": ["arn:aws:ssm:us-east-1:000000000000:opsitem:1"]}),
    );
    w.ok(
        "SetAlarmState",
        &json!({"AlarmName": "a", "StateValue": "ALARM", "StateReason": "x"}),
    );
    assert_eq!(w.core.outbox.len(), 1);
    assert_eq!(
        w.core.outbox[0].action,
        "arn:aws:sns:us-east-1:000000000000:t"
    );
    // Disabled actions queue nothing.
    w.ok("DisableAlarmActions", &json!({"AlarmNames": ["a"]}));
    w.ok(
        "SetAlarmState",
        &json!({"AlarmName": "a", "StateValue": "OK", "StateReason": "x"}),
    );
    assert_eq!(w.core.outbox.len(), 1);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let caller = aws_wire::service::Caller {
        access_key_id: "test".into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: String::new(),
        service: None,
        session_token: None,
    };
    rt.block_on(w.core.deliver(&caller));
    let h = w.ok(
        "DescribeAlarmHistory",
        &json!({"AlarmName": "a", "HistoryItemType": "Action"}),
    );
    let summary = h["AlarmHistoryItems"][0]["HistorySummary"]
        .as_str()
        .unwrap();
    assert!(
        summary.starts_with(
            "Failed to execute action arn:aws:sns:us-east-1:000000000000:t: sns is not running"
        ),
        "{summary}"
    );
}

#[test]
fn tags_and_dashboards_are_stored_and_returned() {
    let mut w = World::new();
    w.alarm("a", &json!({}));
    let arn = "arn:aws:cloudwatch:us-east-1:000000000000:alarm:a";
    w.ok(
        "TagResource",
        &json!({"ResourceARN": arn, "Tags": [{"Key": "env", "Value": "dev"}]}),
    );
    assert_eq!(
        w.ok("ListTagsForResource", &json!({"ResourceARN": arn}))["Tags"],
        json!([{"Key": "env", "Value": "dev"}])
    );
    w.ok(
        "UntagResource",
        &json!({"ResourceARN": arn, "TagKeys": ["env"]}),
    );
    assert_eq!(
        w.ok("ListTagsForResource", &json!({"ResourceARN": arn}))["Tags"],
        json!([])
    );

    let body = r#"{"widgets":[{"type":"text","x":0,"y":0,"width":6,"height":3,"properties":{"markdown":"hi"}}]}"#;
    w.ok(
        "PutDashboard",
        &json!({"DashboardName": "main", "DashboardBody": body}),
    );
    let g = w.ok("GetDashboard", &json!({"DashboardName": "main"}));
    assert_eq!(g["DashboardBody"], body);
    assert_eq!(
        g["DashboardArn"],
        "arn:aws:cloudwatch::000000000000:dashboard/main"
    );
    assert_eq!(
        w.ok("ListDashboards", &json!({}))["DashboardEntries"][0]["DashboardName"],
        "main"
    );
    let (code, _) = w.err(
        "PutDashboard",
        &json!({"DashboardName": "bad", "DashboardBody": "nope"}),
    );
    assert_eq!(code, "DashboardInvalidInputError");
    w.ok("DeleteDashboards", &json!({"DashboardNames": ["main"]}));
    let (code, _) = w.err("GetDashboard", &json!({"DashboardName": "main"}));
    assert_eq!(code, "DashboardNotFoundError");
}

#[test]
fn an_anomaly_detector_is_stored_and_found_again() {
    let mut w = World::new();
    let made = w.ok(
        "PutAnomalyDetector",
        &json!({"SingleMetricAnomalyDetector": {"Namespace": "T", "MetricName": "m", "Stat": "Average"}}),
    );
    let id = made["AnomalyDetectorId"].as_str().unwrap().to_owned();
    let d = w.ok("DescribeAnomalyDetectors", &json!({}));
    assert_eq!(d["AnomalyDetectors"][0]["AnomalyDetectorId"], id);
    assert_eq!(d["AnomalyDetectors"][0]["StateValue"], "PENDING_TRAINING");
    w.ok("DeleteAnomalyDetector", &json!({"AnomalyDetectorId": id}));
    assert_eq!(
        w.ok("DescribeAnomalyDetectors", &json!({}))["AnomalyDetectors"],
        json!([])
    );
}
