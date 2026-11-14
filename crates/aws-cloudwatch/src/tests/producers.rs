//! Data another service holds until it is read (Lambda's `AWS/Lambda` metrics): taken in before a read, and asked for at once while
//! an alarm watches its namespace.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aws_peers::{Made, Peers};
use aws_wire::service::Caller;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use super::temp;
use crate::Cloudwatch;

fn caller() -> Caller {
    Caller {
        access_key_id: "test".into(),
        account_id: "111122223333".into(),
        region: "us-east-1".into(),
        endpoint: String::new(),
        service: None,
        session_token: None,
    }
}

#[tokio::test]
async fn held_data_is_taken_in_before_a_read_and_an_alarm_says_what_it_watches() {
    let dir = temp();
    let peers = Peers::new();
    let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
    let ctx = Arc::new(Ctx {
        dir: dir.path().to_path_buf(),
        idle: Duration::from_secs(300),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: Some(ext),
    });
    peers.register(Arc::new(Cloudwatch::new(ctx)));
    let held = Arc::new(Mutex::new(vec![Made {
        service: "lambda",
        account: "111122223333".into(),
        body: json!({"Namespace": "AWS/Lambda", "MetricData": [
            {"MetricName": "Invocations", "Dimensions": [{"Name": "FunctionName", "Value": "f"}], "Values": [1.0], "Counts": [3.0],
             "Unit": "Count"}]}),
    }]));
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let (h, a) = (Arc::clone(&held), Arc::clone(&asked));
    peers.add_producer(Arc::new(move |region: &str| {
        a.lock().map(|mut v| v.push(region.to_owned())).ok();
        h.lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }));
    let call = |op: &'static str, body: Value| {
        let peers = Arc::clone(&peers);
        async move {
            peers
                .call("test", &caller(), "monitoring", op, &body)
                .await
                .unwrap_or_else(|e| panic!("{op}: {}", e.message))
        }
    };
    // A write is not a read: nothing is asked for.
    call(
        "PutMetricData",
        json!({"Namespace": "T", "MetricData": [{"MetricName": "m", "Value": 1.0}]}),
    )
    .await;
    assert!(asked.lock().map(|v| v.is_empty()).unwrap_or(false));
    // A read takes in what is held.
    let listed = call("ListMetrics", json!({"Namespace": "AWS/Lambda"})).await;
    assert_eq!(listed["Metrics"][0]["MetricName"], "Invocations");
    assert_eq!(
        asked.lock().map(|v| v.clone()).unwrap_or_default(),
        ["us-east-1"]
    );
    // An alarm on the namespace says it is watched (so the producer sends at once); deleting it says it is not.
    assert!(!peers.watched("us-east-1", "AWS/Lambda"));
    call(
        "PutMetricAlarm",
        json!({"AlarmName": "errors", "Namespace": "AWS/Lambda", "MetricName": "Errors", "Statistic": "Sum", "Period": 60,
            "EvaluationPeriods": 1, "Threshold": 1.0, "ComparisonOperator": "GreaterThanOrEqualToThreshold"}),
    )
    .await;
    assert!(peers.watched("us-east-1", "AWS/Lambda"));
    assert!(!peers.watched("eu-west-1", "AWS/Lambda"));
    call("DeleteAlarms", json!({"AlarmNames": ["errors"]})).await;
    assert!(!peers.watched("us-east-1", "AWS/Lambda"));
}
