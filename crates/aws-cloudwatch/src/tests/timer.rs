//! The alarm timer, in real time with an alarm whose period is a second: it sleeps to a deadline and to nothing else, and it is gone
//! when no alarm is pending. (Tokio's paused clock cannot drive it: the actor's save waits on a thread, and with nothing else to wait
//! on the paused clock runs ahead to the idle timeout.)

use std::sync::Arc;
use std::time::Duration;

use aws_testkit::Harness;
use aws_wire::service::Service;
use doze_runtime::{Clock, Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use super::temp;
use crate::Cloudwatch;

struct Rig {
    svc: Cloudwatch,
    h: Harness<Cloudwatch>,
    _dir: aws_testkit::TempDir,
}

impl Rig {
    fn new() -> Self {
        let dir = temp();
        let ctx = Arc::new(Ctx {
            dir: dir.path().to_path_buf(),
            idle: Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        });
        let decoder = Cloudwatch::new(Arc::clone(&ctx));
        Self {
            svc: Cloudwatch::new(ctx),
            h: Harness::new(decoder),
            _dir: dir,
        }
    }

    async fn call(&self, op: &str, request: &Value) -> Value {
        let ix = self.h.op(op);
        let call = self.h.call(ix, request).unwrap();
        let out = self.svc.call(call).await.unwrap();
        self.h.json(ix, &out)
    }

    async fn state(&self) -> String {
        let d = self
            .call("DescribeAlarms", &json!({"AlarmNames": ["a"]}))
            .await;
        d["MetricAlarms"][0]["StateValue"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Waits (in real time, without making the service work) until the alarm is in `want`, for at most `secs` seconds.
    async fn until(&self, want: &str, secs: u64) -> bool {
        for _ in 0..secs * 10 {
            if self.state().await == want {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    async fn timer_gone(&self) -> bool {
        for _ in 0..20 {
            if !self.svc.timer_armed("us-east-1") {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }
}

/// The state of alarm `a` as `svc` reports it.
async fn described(h: &Harness<Cloudwatch>, svc: &Cloudwatch) -> String {
    let ix = h.op("DescribeAlarms");
    let call = h.call(ix, &json!({"AlarmNames": ["a"]})).unwrap();
    let out = svc.call(call).await.unwrap();
    h.json(ix, &out)["MetricAlarms"][0]["StateValue"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn a_restart_judges_again_the_alarms_that_were_pending_without_a_request() {
    let dir = temp();
    let ctx = |tasks: Tasks| {
        Arc::new(Ctx {
            dir: dir.path().to_path_buf(),
            idle: Duration::from_secs(300),
            tasks,
            clock: Arc::new(SystemClock),
            ext: None,
        })
    };
    let before = Tasks::new();
    let one = Cloudwatch::new(ctx(before.clone()));
    let h = Harness::new(Cloudwatch::new(ctx(Tasks::new())));
    let call = |op: &str, req: Value| {
        let ix = h.op(op);
        h.call(ix, &req).unwrap()
    };
    one.call(call(
        "PutMetricAlarm",
        json!({"AlarmName": "a", "Namespace": "T", "MetricName": "m", "Statistic": "Maximum", "Period": 1,
            "EvaluationPeriods": 1, "Threshold": 100.0, "ComparisonOperator": "GreaterThanThreshold",
            "TreatMissingData": "notBreaching"}),
    ))
    .await
    .unwrap();
    let into = SystemClock.now_ms().rem_euclid(1000);
    tokio::time::sleep(Duration::from_millis(u64::try_from(1100 - into).unwrap())).await;
    one.call(call(
        "PutMetricData",
        json!({"Namespace": "T", "MetricData": [{"MetricName": "m", "Value": 150.0, "StorageResolution": 1}]}),
    ))
    .await
    .unwrap();
    // The second holding the datapoint is judged when it is over.
    let mut seen = false;
    for _ in 0..30 {
        if described(&h, &one).await == "ALARM" {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(seen, "the alarm saw the datapoint");
    // The process stops with the alarm in ALARM and a deadline pending.
    before.shutdown().await;
    drop(one);

    let two = Cloudwatch::new(ctx(Tasks::new()));
    two.resume().await;
    // No request is made for a second and a half. The datapoint was in a one-second window, so the alarm's deadline has come: the
    // restarted service's own timer judges it, and the first request after only reports what it finds.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        described(&h, &two).await,
        "OK",
        "the restarted service judged the alarm at its deadline"
    );
}

#[tokio::test]
async fn the_timer_sleeps_to_the_deadline_and_is_gone_when_there_is_none() {
    let rig = Rig::new();
    rig.call(
        "PutMetricAlarm",
        &json!({"AlarmName": "a", "Namespace": "T", "MetricName": "m", "Statistic": "Maximum", "Period": 1,
            "EvaluationPeriods": 1, "Threshold": 100.0, "ComparisonOperator": "GreaterThanThreshold",
            "TreatMissingData": "notBreaching"}),
    )
    .await;
    // The first look is due at once; once taken, nothing can change the alarm without data, so no timer is left.
    assert!(
        rig.until("OK", 3).await,
        "the alarm was judged without a request"
    );
    assert!(rig.timer_gone().await, "no alarm is pending, so no timer");

    // Start of a fresh second, so the data point and its judging fall in the same one.
    let into = SystemClock.now_ms().rem_euclid(1000);
    tokio::time::sleep(Duration::from_millis(u64::try_from(1100 - into).unwrap())).await;
    rig.call(
        "PutMetricData",
        &json!({"Namespace": "T", "MetricData": [{"MetricName": "m", "Value": 150.0, "StorageResolution": 1}]}),
    )
    .await;
    assert!(rig.svc.timer_armed("us-east-1"), "a deadline is pending");
    // The second holding the datapoint is judged when it is over, by the timer.
    assert!(
        rig.until("ALARM", 3).await,
        "the alarm saw the datapoint when its second ended"
    );
    // No request changes anything from here; the second ends, the datapoint leaves the window, and the alarm goes back by itself.
    assert!(
        rig.until("OK", 4).await,
        "the alarm returned to OK at its deadline"
    );
    assert!(rig.timer_gone().await);
}
