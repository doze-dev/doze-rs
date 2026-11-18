//! The alarm timer, in real time with an alarm whose period is ten seconds (the shortest AWS allows): it sleeps to a deadline and to
//! nothing else, and it is gone when no alarm is pending. The clock is the system's moved forward by the test, so a ten-second
//! window can be made to end a moment from now. (Tokio's paused clock cannot drive it: the actor's save waits on a thread, and with
//! nothing else to wait on the paused clock runs ahead to the idle timeout.)

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use aws_testkit::Harness;
use aws_wire::service::Service;
use doze_runtime::{Clock, Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use super::temp;
use crate::Cloudwatch;

/// The system clock, moved forward by what the test says.
#[derive(Debug, Default)]
struct Shifted(AtomicI64);

impl Clock for Shifted {
    fn now_ms(&self) -> i64 {
        SystemClock.now_ms() + self.0.load(Ordering::Relaxed)
    }
}

impl Shifted {
    /// Moves the clock forward until a ten-second boundary is `before_ms` away; returns that boundary.
    fn near_boundary(&self, before_ms: i64) -> i64 {
        let now = self.now_ms();
        let mut shift = 10_000 - now.rem_euclid(10_000) - before_ms;
        if shift < 0 {
            shift += 10_000;
        }
        self.0.fetch_add(shift, Ordering::Relaxed);
        now + shift + before_ms
    }

    /// Sleeps (really) until the clock reads `at`.
    async fn sleep_until(&self, at: i64) {
        let wait = u64::try_from(at - self.now_ms()).unwrap_or(0);
        tokio::time::sleep(Duration::from_millis(wait)).await;
    }
}

const ALARM: &str = r#"{"AlarmName": "a", "Namespace": "T", "MetricName": "m", "Statistic": "Maximum", "Period": 10,
    "EvaluationPeriods": 1, "Threshold": 100.0, "ComparisonOperator": "GreaterThanThreshold", "TreatMissingData": "notBreaching"}"#;

struct Rig {
    svc: Cloudwatch,
    clock: Arc<Shifted>,
    h: Harness<Cloudwatch>,
    _dir: aws_testkit::TempDir,
}

impl Rig {
    fn new() -> Self {
        let dir = temp();
        let clock = Arc::new(Shifted::default());
        let ctx = Arc::new(Ctx {
            dir: dir.path().to_path_buf(),
            idle: Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: clock.clone(),
            ext: None,
        });
        let decoder = Cloudwatch::new(Arc::clone(&ctx));
        Self {
            svc: Cloudwatch::new(ctx),
            clock,
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
    let clock = Arc::new(Shifted::default());
    let ctx = |tasks: Tasks| {
        Arc::new(Ctx {
            dir: dir.path().to_path_buf(),
            idle: Duration::from_secs(300),
            tasks,
            clock: clock.clone(),
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
    one.call(call("PutMetricAlarm", serde_json::from_str(ALARM).unwrap()))
        .await
        .unwrap();
    // A datapoint just before a window ends.
    let boundary = clock.near_boundary(300);
    one.call(call(
        "PutMetricData",
        json!({"Namespace": "T", "MetricData": [{"MetricName": "m", "Value": 150.0, "StorageResolution": 1}]}),
    ))
    .await
    .unwrap();
    // The window holding the datapoint is judged when it is over.
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
    // No request is made until the next window is half a second over. The datapoint has left it, so the alarm's deadline has come:
    // the restarted service's own timer judges it, and the first request after only reports what it finds.
    clock.sleep_until(boundary + 10_500).await;
    assert_eq!(
        described(&h, &two).await,
        "OK",
        "the restarted service judged the alarm at its deadline"
    );
}

#[tokio::test]
async fn the_timer_sleeps_to_the_deadline_and_is_gone_when_there_is_none() {
    let rig = Rig::new();
    rig.call("PutMetricAlarm", &serde_json::from_str(ALARM).unwrap())
        .await;
    // The first look is due at once; once taken, nothing can change the alarm without data, so no timer is left.
    assert!(
        rig.until("OK", 3).await,
        "the alarm was judged without a request"
    );
    assert!(rig.timer_gone().await, "no alarm is pending, so no timer");

    // A datapoint just before a window ends.
    rig.clock.near_boundary(300);
    rig.call(
        "PutMetricData",
        &json!({"Namespace": "T", "MetricData": [{"MetricName": "m", "Value": 150.0, "StorageResolution": 1}]}),
    )
    .await;
    assert!(rig.svc.timer_armed("us-east-1"), "a deadline is pending");
    // The window holding the datapoint is judged when it is over, by the timer.
    assert!(
        rig.until("ALARM", 3).await,
        "the alarm saw the datapoint when its window ended"
    );
    // No request changes anything from here; the next window ends without the datapoint, and the alarm goes back by itself.
    assert!(
        rig.until("OK", 13).await,
        "the alarm returned to OK at its deadline"
    );
    assert!(rig.timer_gone().await);
}
