#![allow(clippy::indexing_slicing)]
use super::*;

fn target(alias: bool) -> Target {
    Target {
        region: "us-east-1".into(),
        account: "111122223333".into(),
        function: "f".into(),
        resource: if alias { "f:live".into() } else { "f".into() },
        executed: alias.then(|| "3".to_owned()),
    }
}

#[test]
fn counts_are_held_by_minute_and_taken_by_region() {
    let m = Meter::default();
    let t = target(false);
    m.add(
        &t,
        60_000,
        &Outcome::Ran {
            duration_ms: 12.5,
            error: false,
        },
    );
    m.add(
        &t,
        61_000,
        &Outcome::Ran {
            duration_ms: 12.5,
            error: true,
        },
    );
    m.add(&t, 62_000, &Outcome::Throttled);
    assert!(m.take("eu-west-1").is_empty());
    let made = m.take("us-east-1");
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].account, "111122223333");
    assert_eq!(made[0].body["Namespace"], "AWS/Lambda");
    let data = made[0].body["MetricData"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // Three dimension sets (none, FunctionName, FunctionName + Resource), four metrics each.
    assert_eq!(data.len(), 12);
    let by_fn: Vec<&Value> = data
        .iter()
        .filter(|d| d["Dimensions"] == json!([{"Name": "FunctionName", "Value": "f"}]))
        .collect();
    let named = |n: &str| by_fn.iter().find(|d| d["MetricName"] == n).copied();
    assert_eq!(
        named("Invocations").map(|d| d["Counts"].clone()),
        Some(json!([2]))
    );
    assert_eq!(
        named("Errors").map(|d| (d["Values"].clone(), d["Counts"].clone())),
        Some((json!([0.0, 1.0]), json!([1, 1])))
    );
    assert_eq!(
        named("Throttles").map(|d| d["Counts"].clone()),
        Some(json!([1]))
    );
    assert_eq!(
        named("Duration").map(|d| (d["Values"].clone(), d["Counts"].clone(), d["Unit"].clone())),
        Some((json!([12.5]), json!([2]), json!("Milliseconds")))
    );
    assert_eq!(by_fn[0]["Timestamp"], 60.0);
    assert!(m.take("us-east-1").is_empty(), "taken once");
}

/// A `provided` function that fails when its event says so.
const BOOTSTRAP: &[u8] = br#"#!/bin/sh
while true; do
  H=$(mktemp)
  EVENT=$(curl -sS -D "$H" "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/next")
  ID=$(grep -i '^Lambda-Runtime-Aws-Request-Id' "$H" | cut -d: -f2 | tr -d ' \r\n')
  rm -f "$H"
  case "$EVENT" in
*fail*) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/error" -d '{"errorMessage":"no","errorType":"Custom"}' ;;
*) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/response" -d '{}' ;;
  esac
done
"#;

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn invocations_reach_cloudwatch_when_it_reads_and_at_once_while_an_alarm_watches() {
    use crate::tests::{World, have};
    if !have("curl") || !have("sh") {
        return;
    }
    let w = World::new();
    let dir = aws_testkit::TempDir::new("lambda-metrics");
    let ext: Arc<dyn std::any::Any + Send + Sync> = w.peers.clone();
    w.peers
        .register(Arc::new(aws_cloudwatch::Cloudwatch::new(Arc::new(
            doze_runtime::Ctx {
                dir: dir.path().to_path_buf(),
                idle: std::time::Duration::from_secs(300),
                tasks: w.tasks.clone(),
                clock: Arc::new(doze_runtime::SystemClock),
                ext: Some(ext),
            },
        ))));
    w.create(
        "m",
        "provided.al2023",
        "bootstrap",
        &[("bootstrap", BOOTSTRAP, 0o755)],
    )
    .await;
    w.invoke("m", &json!({"n": 1})).await;
    w.invoke("m", &json!({"fail": true})).await;
    // Nothing was sent: the counts are held.
    assert!(
        !w.lambda
            .inner
            .meter
            .slots
            .lock()
            .map(|s| s.is_empty())
            .unwrap_or(true)
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let stat = |metric: &str, stat: &str| {
        json!({"Namespace": "AWS/Lambda", "MetricName": metric, "Dimensions": [{"Name": "FunctionName", "Value": "m"}],
            "StartTime": now - 600.0, "EndTime": now + 120.0, "Period": 3600, "Statistics": [stat]})
    };
    // A read takes them in.
    let inv = w
        .peer(
            "monitoring",
            "GetMetricStatistics",
            stat("Invocations", "Sum"),
        )
        .await;
    assert_eq!(inv["Datapoints"][0]["Sum"], 2.0);
    let err = w
        .peer(
            "monitoring",
            "GetMetricStatistics",
            stat("Errors", "Average"),
        )
        .await;
    assert_eq!(err["Datapoints"][0]["Average"], 0.5);
    let dur = w
        .peer(
            "monitoring",
            "GetMetricStatistics",
            stat("Duration", "SampleCount"),
        )
        .await;
    assert_eq!(dur["Datapoints"][0]["SampleCount"], 2.0);
    assert_eq!(dur["Datapoints"][0]["Unit"], "Milliseconds");
    let listed = w
        .peer(
            "monitoring",
            "ListMetrics",
            json!({"Namespace": "AWS/Lambda", "MetricName": "Invocations"}),
        )
        .await;
    assert_eq!(
        listed["Metrics"].as_array().map(Vec::len),
        Some(3),
        "{listed}"
    );
    // With an alarm on the namespace, an invocation is sent as it ends.
    w.peer(
        "monitoring",
        "PutMetricAlarm",
        json!({"AlarmName": "errors", "Namespace": "AWS/Lambda", "MetricName": "Errors", "Statistic": "Sum",
            "Dimensions": [{"Name": "FunctionName", "Value": "m"}], "Period": 60, "EvaluationPeriods": 1,
            "Threshold": 1.0, "ComparisonOperator": "GreaterThanOrEqualToThreshold"}),
    )
    .await;
    w.invoke("m", &json!({"fail": true})).await;
    assert!(
        w.lambda
            .inner
            .meter
            .slots
            .lock()
            .map(|s| s.is_empty())
            .unwrap_or(false),
        "sent at once"
    );
    w.stop().await;
}

#[test]
fn an_alias_adds_the_executed_version_and_the_ring_is_bounded() {
    let m = Meter::default();
    m.add(&target(true), 0, &Outcome::Failed);
    let made = m.take("us-east-1");
    let data = made[0].body["MetricData"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(data.iter().any(|d| d["Dimensions"]
        == json!([{"Name": "FunctionName", "Value": "f"}, {"Name": "Resource", "Value": "f:live"},
            {"Name": "ExecutedVersion", "Value": "3"}])));
    assert!(data.iter().all(|d| d["MetricName"] != "Duration"));
    for minute in 0..i64::try_from(MAX_SLOTS).unwrap_or(0) {
        m.add(&target(false), minute * 60_000, &Outcome::Throttled);
    }
    assert_eq!(m.slots.lock().map(|s| s.len()).unwrap_or(0), MAX_SLOTS);
}
