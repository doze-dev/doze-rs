//! Writing, reading and filtering events.

use serde_json::{Value, json};

use super::{NOW, World};

fn messages(v: &Value) -> Vec<String> {
    v["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["message"].as_str().unwrap().to_owned())
        .collect()
}

fn world() -> World {
    let mut w = World::new();
    w.group("/g");
    w.stream("/g", "app");
    w
}

#[test]
fn events_are_read_back_in_time_order_from_either_end() {
    let mut w = world();
    let put = w.put("/g", "app", NOW, &["a", "b", "c", "d", "e"]);
    assert!(put["nextSequenceToken"].as_str().unwrap().len() >= 50);
    assert!(put.get("rejectedLogEventsInfo").is_none());
    let all = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true}),
    );
    assert_eq!(messages(&all), ["a", "b", "c", "d", "e"]);
    assert_eq!(all["events"][0]["timestamp"], NOW);
    assert_eq!(all["events"][0]["ingestionTime"], NOW);
    // Without startFromHead the newest come back, oldest first among them.
    let tail = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "limit": 2}),
    );
    assert_eq!(messages(&tail), ["d", "e"]);
    // The backward token pages towards the start; the forward token pages on, and at the end returns itself.
    let back = tail["nextBackwardToken"].as_str().unwrap();
    let before = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "limit": 2, "nextToken": back}),
    );
    assert_eq!(messages(&before), ["b", "c"]);
    let fwd = tail["nextForwardToken"].as_str().unwrap();
    let after = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "nextToken": fwd}),
    );
    assert!(messages(&after).is_empty());
    assert_eq!(after["nextForwardToken"], tail["nextForwardToken"]);
    // A time window: start inclusive, end exclusive.
    let win = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true, "startTime": NOW + 1, "endTime": NOW + 3}),
    );
    assert_eq!(messages(&win), ["b", "c"]);
}

#[test]
fn a_batch_must_be_in_order_and_old_and_new_events_are_rejected_with_their_indexes() {
    let mut w = world();
    let (code, msg) = w.err(
        "PutLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "logEvents": [
            {"timestamp": NOW, "message": "b"}, {"timestamp": NOW - 1000, "message": "a"}]}),
    );
    assert_eq!(code, "InvalidParameterException");
    assert_eq!(
        msg,
        "Log events in a single PutLogEvents request must be in chronological order."
    );
    let day = 86_400_000;
    let out = w.ok(
        "PutLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "logEvents": [
            {"timestamp": NOW - 15 * day, "message": "ancient"},
            {"timestamp": NOW, "message": "now"},
            {"timestamp": NOW + 3 * 3_600_000, "message": "future"}]}),
    );
    assert_eq!(out["rejectedLogEventsInfo"]["tooOldLogEventEndIndex"], 1);
    assert_eq!(out["rejectedLogEventsInfo"]["tooNewLogEventStartIndex"], 2);
    let all = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true}),
    );
    assert_eq!(messages(&all), ["now"]);
    // The stream now reports its first and last event.
    let s = w.ok("DescribeLogStreams", &json!({"logGroupName": "/g"}));
    assert_eq!(s["logStreams"][0]["firstEventTimestamp"], NOW);
    assert_eq!(s["logStreams"][0]["lastEventTimestamp"], NOW);
    assert_eq!(s["logStreams"][0]["lastIngestionTime"], NOW);
    assert!(s["logStreams"][0]["uploadSequenceToken"].is_string());
}

#[test]
fn filter_log_events_reads_across_streams_in_time_order_and_pages() {
    let mut w = world();
    w.stream("/g", "worker");
    w.put("/g", "app", NOW, &["INFO started", "ERROR disk full"]);
    w.put("/g", "worker", NOW + 1, &["ERROR timeout"]);
    w.put("/g", "app", NOW + 5, &["ERROR again"]);
    let f = |w: &mut World, req: Value| w.ok("FilterLogEvents", &req);
    let errors = f(
        &mut w,
        json!({"logGroupName": "/g", "filterPattern": "ERROR"}),
    );
    assert_eq!(
        messages(&errors),
        ["ERROR disk full", "ERROR timeout", "ERROR again"]
    );
    assert_eq!(errors["events"][1]["logStreamName"], "worker");
    assert!(errors["events"][0]["eventId"].is_string());
    let one = f(
        &mut w,
        json!({"logGroupName": "/g", "filterPattern": "ERROR", "limit": 2}),
    );
    assert_eq!(messages(&one), ["ERROR disk full", "ERROR timeout"]);
    let rest = f(
        &mut w,
        json!({"logGroupName": "/g", "filterPattern": "ERROR", "limit": 2, "nextToken": one["nextToken"]}),
    );
    assert_eq!(messages(&rest), ["ERROR again"]);
    assert!(rest.get("nextToken").is_none());
    let named = f(
        &mut w,
        json!({"logGroupName": "/g", "logStreamNames": ["worker"]}),
    );
    assert_eq!(messages(&named), ["ERROR timeout"]);
    let prefixed = f(
        &mut w,
        json!({"logGroupName": "/g", "logStreamNamePrefix": "app", "startTime": NOW + 2}),
    );
    assert_eq!(messages(&prefixed), ["ERROR again"]);
}

#[test]
fn events_older_than_the_retention_are_gone_when_the_group_is_next_used() {
    let mut w = world();
    w.ok(
        "PutRetentionPolicy",
        &json!({"logGroupName": "/g", "retentionInDays": 1}),
    );
    w.put("/g", "app", NOW, &["old"]);
    w.clock.advance(2 * 86_400_000);
    let now = NOW + 2 * 86_400_000;
    w.put("/g", "app", now, &["new"]);
    let all = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true}),
    );
    assert_eq!(messages(&all), ["new"]);
    // An event already past the retention is refused on the way in, with its index.
    let out = w.ok(
        "PutLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "logEvents": [
            {"timestamp": now - 2 * 86_400_000, "message": "stale"}, {"timestamp": now, "message": "fresh"}]}),
    );
    assert_eq!(out["rejectedLogEventsInfo"]["expiredLogEventEndIndex"], 1);
}

#[test]
fn stored_bytes_count_each_event_as_its_message_plus_26() {
    let mut w = world();
    w.put("/g", "app", NOW, &["abcd", "xy"]);
    let d = w.ok("DescribeLogGroups", &json!({}));
    assert_eq!(d["logGroups"][0]["storedBytes"], 4 + 26 + 2 + 26);
    w.ok(
        "DeleteLogStream",
        &json!({"logGroupName": "/g", "logStreamName": "app"}),
    );
    assert_eq!(
        w.ok("DescribeLogGroups", &json!({}))["logGroups"][0]["storedBytes"],
        0
    );
}

#[test]
fn a_metric_filter_turns_matching_events_into_data_points() {
    let mut w = world();
    w.ok(
        "PutMetricFilter",
        &json!({"logGroupName": "/g", "filterName": "slow", "filterPattern": "{ $.ms > 100 }",
            "metricTransformations": [{"metricName": "Slow", "metricNamespace": "App", "metricValue": "$.ms",
                "dimensions": {"route": "$.route"}, "unit": "Milliseconds"}]}),
    );
    w.put(
        "/g",
        "app",
        NOW,
        &[r#"{"ms":250,"route":"/a"}"#, r#"{"ms":50,"route":"/b"}"#],
    );
    assert_eq!(w.core.emit.len(), 1);
    let d = &w.core.emit[0];
    assert_eq!(d["Namespace"], "App");
    assert_eq!(d["MetricData"].as_array().unwrap().len(), 1);
    assert_eq!(d["MetricData"][0]["Value"], 250.0);
    assert_eq!(
        d["MetricData"][0]["Dimensions"][0],
        json!({"Name": "route", "Value": "/a"})
    );
}

#[test]
fn a_batch_with_no_match_reports_the_default_value_once_a_minute() {
    let mut w = world();
    w.ok(
        "PutMetricFilter",
        &json!({"logGroupName": "/g", "filterName": "errors", "filterPattern": "ERROR",
            "metricTransformations": [{"metricName": "Errors", "metricNamespace": "App", "metricValue": "1",
                "defaultValue": 0.0, "unit": "Count"}]}),
    );
    w.put("/g", "app", NOW, &["INFO all well"]);
    assert_eq!(w.core.emit.len(), 1);
    let d = &w.core.emit[0];
    assert_eq!(d["Namespace"], "App");
    assert_eq!(
        d["MetricData"],
        json!([{"MetricName": "Errors", "Value": 0.0, "Timestamp": NOW as f64 / 1000.0, "Unit": "Count"}])
    );
    w.core.emit.clear();
    // The same minute: reported already.
    w.clock.advance(10_000);
    w.put("/g", "app", NOW + 10_000, &["INFO still well"]);
    assert!(w.core.emit.is_empty());
    // A match is reported as the value, never beside a default in the same batch.
    w.put("/g", "app", NOW + 10_001, &["ERROR boom"]);
    assert_eq!(w.core.emit.len(), 1);
    assert_eq!(w.core.emit[0]["MetricData"][0]["Value"], 1.0);
    w.core.emit.clear();
    // The next minute with logs and no match reports it again; a filter without a default reports nothing.
    w.clock.advance(60_000);
    w.put("/g", "app", NOW + 70_000, &["INFO later"]);
    assert_eq!(w.core.emit.len(), 1);
    assert_eq!(w.core.emit[0]["MetricData"][0]["Value"], 0.0);
}

#[test]
fn a_filter_with_dimensions_reports_no_default() {
    let mut w = world();
    w.ok(
        "PutMetricFilter",
        &json!({"logGroupName": "/g", "filterName": "f", "filterPattern": "{ $.ms = * }",
            "metricTransformations": [{"metricName": "M", "metricNamespace": "App", "metricValue": "$.ms",
                "defaultValue": 0.0, "dimensions": {"route": "$.route"}}]}),
    );
    w.put("/g", "app", NOW, &["no json here"]);
    assert!(w.core.emit.is_empty());
    // `= *` selects the event whatever the type of the field.
    w.put("/g", "app", NOW + 1, &[r#"{"ms":3,"route":"/a"}"#]);
    assert_eq!(w.core.emit[0]["MetricData"][0]["Value"], 3.0);
}

#[test]
fn embedded_metric_format_lines_lambda_writes_become_data_points() {
    let mut w = world();
    let line = format!(
        r#"{{"_aws":{{"Timestamp":{NOW},"CloudWatchMetrics":[{{"Namespace":"shop","Dimensions":[["service"]],"Metrics":[{{"Name":"latency","Unit":"Milliseconds"}}]}}]}},"service":"cart","latency":42}}"#
    );
    let put = json!({"logGroupName": "/g", "logStreamName": "app", "logEvents": [
        {"timestamp": NOW, "message": "START RequestId: 1 Version: $LATEST\n"},
        {"timestamp": NOW, "message": format!("{line}\n")}]});
    w.run_as(Some("lambda"), "PutLogEvents", &put).unwrap();
    assert_eq!(w.core.emit.len(), 1);
    let d = &w.core.emit[0];
    assert_eq!(d["Namespace"], "shop");
    assert_eq!(d["MetricData"][0]["MetricName"], "latency");
    assert_eq!(d["MetricData"][0]["Values"], json!([42.0]));
    assert_eq!(
        d["MetricData"][0]["Dimensions"],
        json!([{"Name": "service", "Value": "cart"}])
    );
    // The line itself is kept as a log event.
    let all = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true}),
    );
    assert_eq!(messages(&all).len(), 2);
    // A client's PutLogEvents is not read for EMF: AWS needs a header doze is not handed.
    w.core.emit.clear();
    w.ok("PutLogEvents", &put);
    assert!(w.core.emit.is_empty());
}

#[test]
fn the_local_bound_removes_old_events_but_the_group_reports_no_retention() {
    let mut w = world();
    w.put("/g", "app", NOW, &["a week old soon"]);
    // Eight days on, with no retention on the group: the local seven-day bound has removed the event.
    w.clock.advance(8 * 86_400_000);
    let now = NOW + 8 * 86_400_000;
    w.put("/g", "app", now, &["fresh"]);
    let all = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true}),
    );
    assert_eq!(messages(&all), ["fresh"]);
    let g = &w.ok("DescribeLogGroups", &json!({}))["logGroups"][0];
    assert!(g.get("retentionInDays").is_none(), "AWS keeps forever: {g}");
    assert_eq!(g["storedBytes"], 5 + 26);
    // With no local bound (`--logs-retention-days 0`) an old event stays.
    let mut w = world();
    w.core.keep_ms = None;
    w.put("/g", "app", NOW, &["kept"]);
    w.clock.advance(30 * 86_400_000);
    w.put("/g", "app", NOW + 30 * 86_400_000, &["new"]);
    let all = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true}),
    );
    assert_eq!(messages(&all), ["kept", "new"]);
}

#[test]
fn a_group_past_its_event_cap_loses_its_oldest_events() {
    let mut w = world();
    w.stream("/g", "later");
    w.core.max_events = 10;
    let old: Vec<String> = (0..6).map(|i| format!("old {i}")).collect();
    let old: Vec<&str> = old.iter().map(String::as_str).collect();
    w.put("/g", "app", NOW, &old);
    let new: Vec<String> = (0..6).map(|i| format!("new {i}")).collect();
    let new: Vec<&str> = new.iter().map(String::as_str).collect();
    w.put("/g", "later", NOW + 1000, &new);
    // Twelve is past ten: the oldest go until the group holds nine.
    let all = w.ok("FilterLogEvents", &json!({"logGroupName": "/g"}));
    let left = messages(&all);
    assert_eq!(left.len(), 9, "{left:?}");
    assert_eq!(left[0], "old 3");
    let streams = w.ok(
        "DescribeLogStreams",
        &json!({"logGroupName": "/g", "orderBy": "LogStreamName"}),
    );
    assert_eq!(streams["logStreams"][0]["firstEventTimestamp"], NOW + 3);
    assert_eq!(
        w.ok("DescribeLogGroups", &json!({}))["logGroups"][0]["storedBytes"],
        3 * (5 + 26) + 6 * (5 + 26)
    );
}
