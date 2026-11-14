use serde_json::json;

use super::{NOW, World};

fn window(w: &mut World) -> serde_json::Value {
    let _ = w;
    json!({"StartTime": (NOW - 300_000) as f64 / 1000.0, "EndTime": (NOW + 300_000) as f64 / 1000.0, "Period": 60})
}

fn stats(w: &mut World, name: &str, dims: serde_json::Value) -> serde_json::Value {
    let mut req = window(w);
    req["Namespace"] = json!("T");
    req["MetricName"] = json!(name);
    req["Dimensions"] = dims;
    req["Statistics"] = json!(["SampleCount", "Sum", "Average", "Minimum", "Maximum"]);
    w.ok("GetMetricStatistics", &req)
}

#[test]
fn values_read_back_as_the_statistics_the_recording_shows() {
    // test_metric_data_reads_back_as_statistics, 2026-11-09: 1, 2, 6 in one minute.
    let mut w = World::new();
    let dims = json!([{"Name": "queue", "Value": "orders"}]);
    let at = (NOW - 120_000) as f64 / 1000.0;
    let data: Vec<serde_json::Value> = [1.0, 2.0, 6.0]
        .iter()
        .map(|v| json!({"MetricName": "depth", "Dimensions": dims, "Timestamp": at, "Value": v, "Unit": "Count"}))
        .collect();
    w.ok(
        "PutMetricData",
        &json!({"Namespace": "T", "MetricData": data}),
    );
    w.ok(
        "PutMetricData",
        &json!({"Namespace": "T", "MetricData": [{"MetricName": "latency", "Timestamp": at, "Unit": "Milliseconds",
            "StatisticValues": {"SampleCount": 4, "Sum": 100, "Minimum": 10, "Maximum": 40}}]}),
    );
    let d = stats(&mut w, "depth", dims.clone());
    assert_eq!(d["Label"], "depth");
    let p = &d["Datapoints"][0];
    assert_eq!(
        (
            p["SampleCount"].as_f64(),
            p["Sum"].as_f64(),
            p["Average"].as_f64(),
            p["Minimum"].as_f64(),
            p["Maximum"].as_f64()
        ),
        (Some(3.0), Some(9.0), Some(3.0), Some(1.0), Some(6.0))
    );
    assert_eq!(p["Unit"], "Count");
    let l = stats(&mut w, "latency", json!([]));
    assert_eq!(l["Datapoints"][0]["Average"], 25.0);
    assert_eq!(l["Datapoints"][0]["Unit"], "Milliseconds");
    // Dimensions are part of a metric's identity: no dimensions is another metric.
    assert_eq!(stats(&mut w, "depth", json!([]))["Datapoints"], json!([]));

    let listed = w.ok("ListMetrics", &json!({"Namespace": "T"}));
    assert_eq!(listed["Metrics"].as_array().unwrap().len(), 2);
    let by_dim = w.ok("ListMetrics", &json!({"Dimensions": [{"Name": "queue"}]}));
    assert_eq!(by_dim["Metrics"][0]["MetricName"], "depth");

    let got = w.ok(
        "GetMetricData",
        &json!({"MetricDataQueries": [{"Id": "d", "MetricStat": {
            "Metric": {"Namespace": "T", "MetricName": "depth", "Dimensions": dims}, "Period": 60, "Stat": "Sum"}}],
            "StartTime": (NOW - 300_000) as f64 / 1000.0, "EndTime": (NOW + 300_000) as f64 / 1000.0}),
    );
    let r = &got["MetricDataResults"][0];
    assert_eq!(
        (
            r["Id"].as_str(),
            r["Label"].as_str(),
            r["StatusCode"].as_str()
        ),
        (Some("d"), Some("depth"), Some("Complete"))
    );
    assert_eq!(r["Values"][0].as_f64(), Some(9.0));
    assert_eq!(r["Values"].as_array().unwrap().len(), 1);
    assert_eq!(got["Messages"], json!([]));
}

#[test]
fn periods_fold_and_a_percentile_comes_from_the_values() {
    let mut w = World::new();
    for (i, v) in (1..=10).enumerate() {
        w.put(
            "m",
            NOW - 180_000 + i64::try_from(i).unwrap() * 30_000,
            f64::from(v),
        );
    }
    let mut req = window(&mut w);
    req["Namespace"] = json!("T");
    req["MetricName"] = json!("m");
    req["Statistics"] = json!(["Sum"]);
    req["ExtendedStatistics"] = json!(["p90"]);
    req["Period"] = json!(120);
    let d = w.ok("GetMetricStatistics", &req);
    let sums: Vec<f64> = d["Datapoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["Sum"].as_f64().unwrap())
        .collect();
    assert_eq!(sums.iter().sum::<f64>(), 55.0);
    assert!(d["Datapoints"][0]["ExtendedStatistics"]["p90"].is_number());
    // Newest first by default in GetMetricData, oldest first with ScanBy.
    let q = |scan: &str| {
        json!({"MetricDataQueries": [{"Id": "q", "MetricStat": {
            "Metric": {"Namespace": "T", "MetricName": "m"}, "Period": 60, "Stat": "Sum"}}],
            "StartTime": (NOW - 300_000) as f64 / 1000.0, "EndTime": (NOW + 300_000) as f64 / 1000.0, "ScanBy": scan})
    };
    let desc = w.ok("GetMetricData", &q("TimestampDescending"));
    let asc = w.ok("GetMetricData", &q("TimestampAscending"));
    let mut reversed = asc["MetricDataResults"][0]["Values"]
        .as_array()
        .unwrap()
        .clone();
    reversed.reverse();
    assert_eq!(
        desc["MetricDataResults"][0]["Values"].as_array().unwrap(),
        &reversed
    );
}

#[test]
fn the_refusals_are_worded_as_aws_worded_them() {
    let mut w = World::new();
    let both = json!({"Namespace": "T", "MetricData": [{"MetricName": "m", "Value": 1,
        "StatisticValues": {"SampleCount": 1, "Sum": 1, "Minimum": 1, "Maximum": 1}}]});
    let (code, msg) = w.err("PutMetricData", &both);
    assert_eq!(code, "InvalidParameterCombinationException");
    assert_eq!(
        msg,
        "The parameters MetricData.member.1.Value and MetricData.member.1.StatisticValues are mutually exclusive and you have specified both."
    );

    let mut req = window(&mut w);
    req["Namespace"] = json!("T");
    req["MetricName"] = json!("m");
    req["Statistics"] = json!(["Sum"]);
    req["Period"] = json!(45);
    let (code, msg) = w.err("GetMetricStatistics", &req);
    assert_eq!(
        (code.as_str(), msg.as_str()),
        (
            "InvalidParameterValueException",
            "The parameter Period must be a value in the set [1, 5, 10, 20, 30]."
        )
    );
    req["Period"] = json!(60);
    req["StartTime"] = req["EndTime"].clone();
    let (_, msg) = w.err("GetMetricStatistics", &req);
    assert_eq!(
        msg,
        "The parameter StartTime must be less than the parameter EndTime."
    );
    let mut none = window(&mut w);
    none["Namespace"] = json!("T");
    none["MetricName"] = json!("m");
    let (code, msg) = w.err("GetMetricStatistics", &none);
    assert_eq!(code, "InvalidParameterCombinationException");
    assert_eq!(
        msg,
        "At least one of the parameters Statistics and ExtendedStatistics must be specified."
    );
    // The reserved AWS/ prefix was accepted by AWS (recorded).
    w.ok(
        "PutMetricData",
        &json!({"Namespace": "AWS/Mine", "MetricData": [{"MetricName": "m", "Value": 1}]}),
    );
}

#[test]
fn metric_math_is_refused_not_guessed() {
    let mut w = World::new();
    let e = w
        .run(
            "GetMetricData",
            &json!({"MetricDataQueries": [{"Id": "e", "Expression": "m1 * 2"}],
                "StartTime": (NOW - 60_000) as f64 / 1000.0, "EndTime": (NOW + 60_000) as f64 / 1000.0}),
        )
        .unwrap_err();
    assert_eq!(e.status, 501);
}

#[test]
fn old_samples_and_quiet_metrics_are_forgotten_and_the_series_count_is_bounded() {
    let mut w = World::new();
    w.put("m", NOW, 1.0);
    w.clock.advance(2 * 86_400_000);
    let later = NOW + 2 * 86_400_000;
    w.put("m", later, 2.0);
    let mut req = json!({"Namespace": "T", "MetricName": "m", "Period": 60, "Statistics": ["Sum"],
        "StartTime": (NOW - 60_000) as f64 / 1000.0, "EndTime": (later + 60_000) as f64 / 1000.0});
    req["Period"] = json!(3600);
    let d = w.ok("GetMetricStatistics", &req);
    let total: f64 = d["Datapoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["Sum"].as_f64().unwrap())
        .sum();
    assert_eq!(
        total, 2.0,
        "the sample from two days ago was removed when the metric was next written"
    );
}

#[test]
fn the_retention_setting_bounds_what_is_kept() {
    let ask = |w: &mut World| {
        w.ok(
            "GetMetricStatistics",
            &json!({"Namespace": "T", "MetricName": "old", "Statistics": ["Sum"], "Period": 60,
                "StartTime": (NOW - 3 * 3_600_000) as f64 / 1000.0, "EndTime": NOW as f64 / 1000.0}),
        )["Datapoints"]
            .as_array()
            .map_or(0, Vec::len)
    };
    let put = |w: &mut World| {
        w.ok(
            "PutMetricData",
            &json!({"Namespace": "T", "MetricData": [{"MetricName": "old", "Value": 1.0,
                "Timestamp": (NOW - 2 * 3_600_000) as f64 / 1000.0}]}),
        );
    };
    // The default keeps a day: a point two hours old is there.
    let mut w = World::new();
    put(&mut w);
    assert_eq!(ask(&mut w), 1);
    // Kept for an hour (`--cloudwatch-retention-hours 1`), the same point is gone once the store is trimmed on the write.
    let mut w = World::new();
    w.core.keep_ms = 3_600_000;
    put(&mut w);
    assert_eq!(ask(&mut w), 0);
}
