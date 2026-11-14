//! The Embedded Metric Format: a log event that is a JSON object with an `_aws` member saying which of its members are metrics
//! becomes CloudWatch data points, as AWS does for what a Lambda function prints (Powertools' metrics, the EMF client libraries).
//!
//! ```json
//! {"_aws": {"Timestamp": 1700000000000, "CloudWatchMetrics": [{"Namespace": "shop", "Dimensions": [["service"]],
//!   "Metrics": [{"Name": "latency", "Unit": "Milliseconds"}]}]}, "service": "cart", "latency": [12, 20]}
//! ```
//!
//! Each directive yields, for every dimension set and every metric, one data point: the metric member's number (or the numbers of an
//! array of up to 100), the set's dimensions read from the members they name, at `_aws.Timestamp`. A directive that does not follow
//! the specification is skipped and the event stays a plain log event, as on AWS; a metric whose member is missing or not a number
//! is skipped. The rules are from AWS's EMF specification and memory; no recording exists. Not applied: the specification's
//! limits on names and the dimension set's size beyond 30.
//!
//! Which events are read: those Lambda writes (AWS extracts from a function's log). A `PutLogEvents` from a client needs the
//! `x-amzn-logs-format: json/emf` header on AWS, which the gateway does not hand a service, so a client's events are not read.

use serde_json::{Map, Value, json};

/// Metrics one directive may name, and dimensions one set may hold (AWS: 100 and 30).
const MAX_METRICS: usize = 100;
const MAX_DIMENSIONS: usize = 30;
/// Values one metric member may hold.
const MAX_VALUES: usize = 100;
/// How far back and ahead a data point may be for CloudWatch to take it (two weeks, two hours).
const PAST_MS: i64 = 14 * 86_400_000;
const FUTURE_MS: i64 = 2 * 3_600_000;

/// The data points of one event, by namespace: `(namespace, datum)` in the shape `PutMetricData` takes. Empty for an event that is
/// not EMF.
pub(crate) fn data_points(message: &str, now_ms: i64) -> Vec<(String, Value)> {
    let text = message.trim();
    if !text.starts_with('{') || !text.contains("\"_aws\"") {
        return Vec::new();
    }
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(aws) = root.get("_aws").and_then(Value::as_object) else {
        return Vec::new();
    };
    let Some(ts) = aws.get("Timestamp").and_then(Value::as_i64) else {
        return Vec::new();
    };
    if ts < now_ms - PAST_MS || ts > now_ms + FUTURE_MS {
        return Vec::new();
    }
    let Some(directives) = aws.get("CloudWatchMetrics").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for d in directives {
        out.extend(directive(&root, d, ts));
    }
    out
}

/// The directive's dimension sets, each as `PutMetricData` dimensions. Each set is a list of member names; an empty list (or no
/// list) is a data point with no dimensions. `None` when a set does not follow the specification.
fn dimension_sets(root: &Map<String, Value>, d: &Value) -> Option<Vec<Vec<Value>>> {
    let mut sets: Vec<Vec<Value>> = Vec::new();
    for set in d
        .get("Dimensions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let names = set.as_array().filter(|s| s.len() <= MAX_DIMENSIONS)?;
        let mut dims = Vec::new();
        for n in names {
            let name = n.as_str()?;
            // A dimension names a member whose value is a string.
            let value = root.get(name).and_then(Value::as_str)?;
            dims.push(json!({"Name": name, "Value": value}));
        }
        sets.push(dims);
    }
    if sets.is_empty() {
        sets.push(Vec::new());
    }
    Some(sets)
}

fn directive(root: &Map<String, Value>, d: &Value, ts: i64) -> Vec<(String, Value)> {
    let Some(namespace) = d
        .get("Namespace")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty() && n.len() <= 255)
    else {
        return Vec::new();
    };
    let Some(metrics) = d
        .get("Metrics")
        .and_then(Value::as_array)
        .filter(|m| m.len() <= MAX_METRICS)
    else {
        return Vec::new();
    };
    let Some(sets) = dimension_sets(root, d) else {
        return Vec::new();
    };
    #[allow(clippy::cast_precision_loss)]
    let at = ts as f64 / 1000.0;
    let mut out = Vec::new();
    for m in metrics {
        let Some(name) = m.get("Name").and_then(Value::as_str) else {
            continue;
        };
        let values: Vec<f64> = match root.get(name) {
            Some(Value::Number(n)) => n.as_f64().into_iter().collect(),
            Some(Value::Array(a)) if !a.is_empty() && a.len() <= MAX_VALUES => {
                let v: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
                if v.len() != a.len() {
                    continue;
                }
                v
            }
            _ => continue,
        };
        for dims in &sets {
            let mut datum = json!({"MetricName": name, "Timestamp": at, "Values": values});
            if let (Some(o), false) = (datum.as_object_mut(), dims.is_empty()) {
                o.insert("Dimensions".into(), Value::Array(dims.clone()));
            }
            if let (Some(o), Some(unit)) =
                (datum.as_object_mut(), m.get("Unit").and_then(Value::as_str))
            {
                o.insert("Unit".into(), json!(unit));
            }
            if let (Some(o), Some(1)) = (
                datum.as_object_mut(),
                m.get("StorageResolution").and_then(Value::as_i64),
            ) {
                o.insert("StorageResolution".into(), json!(1));
            }
            out.push((namespace.to_owned(), datum));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    #[test]
    fn a_directive_becomes_a_point_per_metric_and_dimension_set() {
        let line = r#"{"_aws":{"Timestamp":1700000000000,"CloudWatchMetrics":[{"Namespace":"shop","Dimensions":[["service"],["service","stage"]],
            "Metrics":[{"Name":"latency","Unit":"Milliseconds"},{"Name":"orders","Unit":"Count","StorageResolution":1}]}]},
            "service":"cart","stage":"dev","latency":[12,20],"orders":1,"note":"x"}"#;
        let points = data_points(&format!("{line}\n"), NOW);
        assert_eq!(points.len(), 4);
        assert!(points.iter().all(|(ns, _)| ns == "shop"));
        let (_, first) = &points[0];
        assert_eq!(first["MetricName"], "latency");
        assert_eq!(first["Values"], json!([12.0, 20.0]));
        assert_eq!(first["Unit"], "Milliseconds");
        assert_eq!(
            first["Dimensions"],
            json!([{"Name": "service", "Value": "cart"}])
        );
        assert_eq!(first["Timestamp"], 1_700_000_000.0);
        assert_eq!(points[1].1["Dimensions"].as_array().map(Vec::len), Some(2));
        assert_eq!(points[2].1["StorageResolution"], 1);
    }

    #[test]
    fn what_does_not_follow_the_specification_is_a_plain_log_line() {
        for line in [
            "plain text",
            r#"{"a":1}"#,
            r#"{"_aws":{"CloudWatchMetrics":[{"Namespace":"n","Dimensions":[[]],"Metrics":[{"Name":"m"}]}]},"m":1}"#,
            r#"{"_aws":{"Timestamp":1700000000000,"CloudWatchMetrics":[{"Dimensions":[[]],"Metrics":[{"Name":"m"}]}]},"m":1}"#,
            r#"{"_aws":{"Timestamp":1700000000000,"CloudWatchMetrics":[{"Namespace":"n","Dimensions":[["missing"]],"Metrics":[{"Name":"m"}]}]},"m":1}"#,
            r#"{"_aws":{"Timestamp":1600000000000,"CloudWatchMetrics":[{"Namespace":"n","Dimensions":[[]],"Metrics":[{"Name":"m"}]}]},"m":1}"#,
        ] {
            assert!(data_points(line, NOW).is_empty(), "{line}");
        }
        // A metric whose member is not a number is skipped; the others stand. No dimension set is one with none.
        let line = r#"{"_aws":{"Timestamp":1700000000000,"CloudWatchMetrics":[{"Namespace":"n","Metrics":[{"Name":"a"},{"Name":"b"}]}]},"a":"x","b":2}"#;
        let points = data_points(line, NOW);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].1["MetricName"], "b");
        assert!(points[0].1.get("Dimensions").is_none());
    }
}
