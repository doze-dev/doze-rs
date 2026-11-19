//! Metric math in `GetMetricData` and in alarms, and log alarms.

use serde_json::{Value, json};

use super::{NOW, World};

fn stat(id: &str, metric: &str, stat: &str) -> Value {
    json!({"Id": id, "ReturnData": false, "MetricStat": {"Metric": {"Namespace": "T", "MetricName": metric}, "Period": 60, "Stat": stat}})
}

fn expr(id: &str, e: &str) -> Value {
    json!({"Id": id, "Expression": e})
}

fn data(w: &mut World, queries: Vec<Value>) -> Result<Value, aws_wire::ApiError> {
    w.run(
        "GetMetricData",
        &json!({"MetricDataQueries": queries, "ScanBy": "TimestampAscending",
            "StartTime": (NOW - 300_000) as f64 / 1000.0, "EndTime": (NOW + 60_000) as f64 / 1000.0}),
    )
}

/// The values of each result, by id.
fn values(v: &Value) -> Vec<(String, Vec<f64>)> {
    v["MetricDataResults"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["Id"].as_str().unwrap().to_owned(),
                r["Values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap())
                    .collect(),
            )
        })
        .collect()
}

fn seed(w: &mut World) {
    for (k, (a, b)) in [(10.0, 4.0), (20.0, 5.0), (40.0, 8.0)].iter().enumerate() {
        let at = NOW - 240_000 + 60_000 * i64::try_from(k).unwrap();
        w.put("a", at, *a);
        w.put("b", at, *b);
    }
    // A gap at NOW - 60s, then one more value of a.
    w.put("a", NOW, 70.0);
}

#[test]
fn expressions_are_worked_out_over_the_queries() {
    let mut w = World::new();
    seed(&mut w);
    let out = data(
        &mut w,
        vec![
            stat("m1", "a", "Sum"),
            stat("m2", "b", "Sum"),
            expr("ratio", "m1 / m2"),
            expr("total", "SUM(METRICS())"),
            expr("mean", "AVG(m1)"),
            expr("filled", "FILL(m1, 0)"),
            expr("rate", "RATE(m1)"),
            expr("cond", "IF(m1 > 15, m1, -1)"),
            expr("chain", "ratio * 2"),
        ],
    )
    .unwrap();
    let v = values(&out);
    let of = |id: &str| v.iter().find(|(i, _)| i == id).unwrap().1.clone();
    assert!(
        v.iter().all(|(id, _)| id != "m1"),
        "ReturnData false is not returned"
    );
    assert_eq!(of("ratio"), [2.5, 4.0, 5.0]);
    assert_eq!(of("total"), [14.0, 25.0, 48.0, 70.0]);
    assert_eq!(
        of("mean"),
        [35.0; 4],
        "a scalar is shown at each timestamp the metrics have"
    );
    assert_eq!(of("filled"), [0.0, 10.0, 20.0, 40.0, 0.0, 70.0]);
    // Per second between datapoints: 10 over a minute, 20 over a minute, 30 over two minutes.
    assert_eq!(of("rate"), [10.0 / 60.0, 20.0 / 60.0, 30.0 / 120.0]);
    assert_eq!(of("cond"), [-1.0, 20.0, 40.0, 70.0]);
    assert_eq!(of("chain"), [5.0, 8.0, 10.0]);
    let label = out["MetricDataResults"][0]["Label"].as_str().unwrap();
    assert_eq!(
        label, "ratio",
        "an expression with no label is labelled with its id"
    );
}

#[test]
fn search_finds_the_metrics_this_region_holds() {
    let mut w = World::new();
    for (q, v) in [("orders", 1.0), ("billing", 2.0)] {
        w.ok(
            "PutMetricData",
            &json!({"Namespace": "T", "MetricData": [{"MetricName": "depth", "Value": v,
                "Timestamp": (NOW - 60_000) as f64 / 1000.0, "Dimensions": [{"Name": "queue", "Value": q}]}]}),
        );
    }
    w.put("depth", NOW - 60_000, 9.0);
    let out = data(
        &mut w,
        vec![expr(
            "s",
            "SEARCH('{T,queue} MetricName=\"depth\"', 'Sum', 60)",
        )],
    )
    .unwrap();
    let results = out["MetricDataResults"].as_array().unwrap();
    assert_eq!(
        results.len(),
        2,
        "the metric with no dimensions is not in the schema"
    );
    let labels: Vec<&str> = results
        .iter()
        .map(|r| r["Label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["depth billing", "depth orders"]);
    let sum = data(
        &mut w,
        vec![expr("s", "SUM(SEARCH('{T,queue} depth', 'Sum', 60))")],
    )
    .unwrap();
    assert_eq!(values(&sum), [("s".to_owned(), vec![3.0])]);
}

#[test]
fn what_cannot_be_worked_out_is_refused() {
    let mut w = World::new();
    for (queries, words) in [
        (vec![expr("e", "m9 * 2")], "Unrecognized identifier m9"),
        (
            vec![expr("e", "f"), expr("f", "e + 1")],
            "Circular dependency",
        ),
        (vec![expr("e", "m1 +")], "Error in expression e"),
        (vec![expr("e", "NOPE(1)")], "Unknown function NOPE"),
        (vec![expr("e", "SELECT AVG(x) FROM T")], "Metrics Insights"),
        (vec![expr("e", "'text'")], "not a time series"),
        (
            vec![
                json!({"Id": "e", "Expression": "1", "MetricStat": {"Metric": {"Namespace": "T", "MetricName": "a"}, "Period": 60, "Stat": "Sum"}}),
            ],
            "either a MetricStat or an Expression",
        ),
    ] {
        let e = data(&mut w, queries).unwrap_err();
        assert_eq!(e.shape.as_deref(), Some("ValidationException"), "{words}");
        assert!(e.message.contains(words), "{} / {words}", e.message);
    }
}

fn math_alarm(w: &mut World, name: &str, extra: &Value) -> Result<Value, aws_wire::ApiError> {
    let mut req = json!({
        "AlarmName": name, "EvaluationPeriods": 1, "Threshold": 100.0, "ComparisonOperator": "GreaterThanThreshold",
        "TreatMissingData": "notBreaching",
        "Metrics": [stat("m1", "a", "Maximum"), stat("m2", "b", "Maximum"), {"Id": "e", "Expression": "m1 + m2", "ReturnData": true}],
    });
    for (k, v) in extra.as_object().unwrap() {
        req[k] = v.clone();
    }
    w.run("PutMetricAlarm", &req)
}

#[test]
fn a_math_alarm_follows_its_expression() {
    let mut w = World::new();
    math_alarm(&mut w, "sum", &json!({})).unwrap();
    assert_eq!(w.core.earliest_due(), Some(NOW));
    w.core.tick();
    assert_eq!(w.state("sum"), "OK");
    // Each metric alone stays under the threshold; their sum does not.
    w.clock.advance(10_000);
    w.put("a", NOW + 10_000, 60.0);
    w.put("b", NOW + 10_000, 50.0);
    assert_eq!(
        w.core.earliest_due(),
        Some(NOW + 60_000),
        "judged when the minute is over"
    );
    w.clock.advance(50_000);
    w.core.tick();
    let d = w.ok("DescribeAlarms", &json!({"AlarmNames": ["sum"]}));
    let a = &d["MetricAlarms"][0];
    assert_eq!(a["StateValue"], "ALARM");
    assert!(
        a["StateReason"].as_str().unwrap().contains("[110.0 ("),
        "{}",
        a["StateReason"]
    );
    assert_eq!(a["Metrics"][2]["Expression"], "m1 + m2");
    // And back when the minute leaves the window.
    assert_eq!(w.core.earliest_due(), Some(NOW + 120_000));
    w.clock.advance(60_000);
    w.core.tick();
    assert_eq!(w.state("sum"), "OK");
}

#[test]
fn a_band_alarm_judges_against_the_band() {
    let mut w = World::new();
    for k in 0..10 {
        w.put(
            "a",
            NOW - 600_000 + k * 60_000,
            if k % 2 == 0 { 10.0 } else { 12.0 },
        );
    }
    math_alarm(
        &mut w,
        "band",
        &json!({"ComparisonOperator": "LessThanLowerOrGreaterThanUpperThreshold", "ThresholdMetricId": "ad",
            "Metrics": [stat("m1", "a", "Maximum"), {"Id": "ad", "Expression": "ANOMALY_DETECTION_BAND(m1, 2)", "ReturnData": false},
                        {"Id": "e", "Expression": "m1", "ReturnData": true}]}),
    )
    .unwrap();
    w.core.tick();
    assert_eq!(w.state("band"), "OK");
    w.clock.advance(10_000);
    w.put("a", NOW + 10_000, 500.0);
    w.clock.advance(50_000);
    w.core.tick();
    assert_eq!(w.state("band"), "ALARM");
}

#[test]
fn a_math_alarm_is_held_to_the_rules_of_the_model() {
    let mut w = World::new();
    let two = json!({"Metrics": [stat("m1", "a", "Sum"), {"Id": "e", "Expression": "m1", "ReturnData": true}, {"Id": "f", "Expression": "m1"}]});
    for (extra, words) in [
        (two, "Exactly one element"),
        (
            json!({"Namespace": "T"}),
            "cannot be used with the parameter Metrics",
        ),
        (
            json!({"Metrics": [{"Id": "e", "Expression": "SEARCH('x', 'Sum', 60)", "ReturnData": true}]}),
            "SEARCH cannot be used in an alarm",
        ),
        (
            json!({"Metrics": [stat("m1", "a", "Sum"), {"Id": "e", "Expression": "METRICS()", "ReturnData": true}]}),
            "single time series",
        ),
        (
            json!({"ComparisonOperator": "GreaterThanUpperThreshold"}),
            "needs a ThresholdMetricId",
        ),
        (
            json!({"ThresholdMetricId": "e"}),
            "cannot be used with the comparison",
        ),
    ] {
        let e = math_alarm(&mut w, "x", &extra).unwrap_err();
        assert!(e.message.contains(words), "{} / {words}", e.message);
    }
}

fn log_alarm(name: &str) -> Value {
    json!({
        "AlarmName": name, "Threshold": 5.0, "ComparisonOperator": "GreaterThanThreshold",
        "QueryResultsToEvaluate": 3, "QueryResultsToAlarm": 2,
        "ScheduledQueryConfiguration": {
            "QueryString": "fields @message | filter @message like /ERROR/", "LogGroupIdentifiers": ["/app"],
            "ScheduledQueryRoleARN": "arn:aws:iam::000000000000:role/q", "AggregationExpression": "count(*)",
            "ScheduleConfiguration": {"ScheduleExpression": "rate(5 minutes)", "StartTimeOffset": 300}},
    })
}

#[test]
fn a_log_alarm_is_kept_and_says_it_is_not_evaluated() {
    let mut w = World::new();
    w.ok("PutLogAlarm", &log_alarm("errors"));
    let all = w.ok("DescribeAlarms", &json!({}));
    assert_eq!(
        all["LogAlarms"],
        json!([]),
        "only metric alarms when no type is asked for"
    );
    let d = w.ok("DescribeAlarms", &json!({"AlarmTypes": ["LogAlarm"]}));
    let a = &d["LogAlarms"][0];
    assert_eq!(a["AlarmName"], "errors");
    assert_eq!(a["StateValue"], "INSUFFICIENT_DATA");
    assert!(
        a["StateReason"]
            .as_str()
            .unwrap()
            .starts_with("Not evaluated locally")
    );
    assert_eq!(a["QueryResultsToAlarm"], 2);
    assert_eq!(a["TreatMissingData"], "missing");
    assert!(
        a["ScheduledQueryConfiguration"]["QueryARN"]
            .as_str()
            .unwrap()
            .starts_with("arn:aws:logs:")
    );
    assert_eq!(d["MetricAlarms"], json!([]));
    let h = w.ok("DescribeAlarmHistory", &json!({"AlarmName": "errors"}));
    assert_eq!(h["AlarmHistoryItems"][0]["AlarmType"], "LogAlarm");
    assert_eq!(w.core.earliest_due(), None);

    let mut bad = log_alarm("x");
    bad["QueryResultsToAlarm"] = json!(4);
    assert!(
        w.err("PutLogAlarm", &bad)
            .1
            .contains("less than or equal to QueryResultsToEvaluate")
    );
    let mut bad = log_alarm("x");
    bad["ScheduledQueryConfiguration"]["ScheduleConfiguration"]["ScheduleExpression"] =
        json!("cron(0 * * * ? *)");
    assert!(w.err("PutLogAlarm", &bad).1.contains("rate()"));
    let mut bad = log_alarm("x");
    bad["ComparisonOperator"] = json!("GreaterThanUpperThreshold");
    assert!(
        w.err("PutLogAlarm", &bad)
            .1
            .contains("not valid for a log alarm")
    );
    w.alarm("metric", &json!({}));
    assert_eq!(
        w.err("PutLogAlarm", &log_alarm("metric")).0,
        "ResourceConflict"
    );
    w.ok("DeleteAlarms", &json!({"AlarmNames": ["errors"]}));
    let d = w.ok("DescribeAlarms", &json!({"AlarmTypes": ["LogAlarm"]}));
    assert_eq!(d["LogAlarms"], json!([]));
}

#[test]
fn a_composite_alarm_is_stored_and_says_it_is_not_evaluated() {
    let mut w = World::new();
    w.ok(
        "PutCompositeAlarm",
        &json!({"AlarmName": "c", "AlarmRule": "ALARM(m)"}),
    );
    let d = w.ok("DescribeAlarms", &json!({"AlarmTypes": ["CompositeAlarm"]}));
    assert_eq!(d["CompositeAlarms"][0]["AlarmRule"], "ALARM(m)");
    assert!(
        d["CompositeAlarms"][0]["StateReason"]
            .as_str()
            .unwrap()
            .starts_with("Not evaluated locally")
    );
    assert_eq!(d["MetricAlarms"], json!([]));
    assert_eq!(w.core.earliest_due(), None);
}
