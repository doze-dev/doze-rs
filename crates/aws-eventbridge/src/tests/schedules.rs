//! Schedules: rate and cron rules fire at their slots and only then, the actor asks for exactly the next deadline, and with nothing
//! scheduled there is no deadline and no timer.

use std::sync::Arc;
use std::time::Duration;

use aws_peers::Peers;
use aws_testkit::Harness;
use aws_wire::service::Service;
use doze_runtime::{Clock, Ctx, Tasks};
use serde_json::{Value, json};

use super::{START, World, caller};
use crate::Eventbridge;

const MINUTE: i64 = 60_000;

/// A default-bus rule on a schedule with a queue to deliver to; returns the rule's ARN.
fn scheduled(w: &mut World, expression: &str) -> String {
    let rule = w.ok(
        "PutRule",
        &json!({"Name": "tick", "ScheduleExpression": expression}),
    )["RuleArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let queue = w.queue("q", &rule);
    w.ok(
        "PutTargets",
        &json!({"Rule": "tick", "Targets": [{"Id": "t", "Arn": queue}]}),
    );
    rule
}

#[test]
fn a_rate_rule_fires_every_period_from_its_creation() {
    let mut w = World::new();
    let rule = scheduled(&mut w, "rate(5 minutes)");
    let due = w.core.next_due();
    assert_eq!(due, Some(START + 5 * MINUTE));
    assert!(w.drain("q").is_empty());
    let next = w.wake_at(START + 5 * MINUTE);
    assert_eq!(next, Some(START + 10 * MINUTE));
    let got = w.bodies("q");
    assert_eq!(got.len(), 1);
    let e = &got[0];
    assert_eq!(
        (e["source"].as_str(), e["detail-type"].as_str()),
        (Some("aws.events"), Some("Scheduled Event"))
    );
    assert_eq!(e["resources"], json!([rule]));
    assert_eq!(e["detail"], json!({}));
    assert_eq!(
        e["time"], "2023-11-14T22:18:00Z",
        "the time the slot was due"
    );
}

#[test]
fn a_long_pause_fires_once_and_does_not_replay_what_was_missed() {
    let mut w = World::new();
    scheduled(&mut w, "rate(5 minutes)");
    w.core.next_due();
    let next = w.wake_at(START + 62 * MINUTE);
    assert_eq!(w.drain("q").len(), 1, "one slot, not twelve");
    assert_eq!(next, Some(START + 65 * MINUTE));
}

#[test]
fn a_cron_rule_fires_at_its_minutes_in_utc() {
    let mut w = World::new();
    scheduled(&mut w, "cron(0/15 * * * ? *)");
    // START is 22:13 on 2023-11-14; the next quarter hour is 22:15.
    assert_eq!(w.core.next_due(), Some(START + 2 * MINUTE));
    let next = w.wake_at(START + 2 * MINUTE);
    assert_eq!(next, Some(START + 17 * MINUTE));
    assert_eq!(w.bodies("q")[0]["time"], "2023-11-14T22:15:00Z");
}

#[test]
fn a_rule_that_cannot_fire_asks_for_no_deadline() {
    let mut w = World::new();
    // No target.
    w.ok(
        "PutRule",
        &json!({"Name": "tick", "ScheduleExpression": "rate(1 minute)"}),
    );
    assert_eq!(w.core.next_due(), None);
    let q = w.queue("q", "arn:aws:events:us-east-1:000000000000:rule/tick");
    w.ok(
        "PutTargets",
        &json!({"Rule": "tick", "Targets": [{"Id": "t", "Arn": q}]}),
    );
    assert!(w.core.next_due().is_some());
    w.ok("DisableRule", &json!({"Name": "tick"}));
    assert_eq!(w.core.next_due(), None, "disabled");
    w.ok("EnableRule", &json!({"Name": "tick"}));
    assert!(w.core.next_due().is_some());
    w.ok("RemoveTargets", &json!({"Rule": "tick", "Ids": ["t"]}));
    assert_eq!(w.core.next_due(), None, "nothing to send to");
    // A rule with a pattern has no schedule to wait for.
    w.rule("default", "pat", &json!({"source": ["x"]}));
    assert_eq!(w.core.next_due(), None);
}

#[test]
fn changing_the_schedule_moves_the_deadline_and_a_cron_that_never_comes_ends() {
    let mut w = World::new();
    scheduled(&mut w, "rate(10 minutes)");
    assert_eq!(w.core.next_due(), Some(START + 10 * MINUTE));
    w.clock.advance(3 * MINUTE);
    w.ok(
        "PutRule",
        &json!({"Name": "tick", "ScheduleExpression": "rate(1 minute)"}),
    );
    assert_eq!(
        w.core.next_due(),
        Some(START + 4 * MINUTE),
        "counted from the change"
    );
    w.ok(
        "PutRule",
        &json!({"Name": "tick", "ScheduleExpression": "cron(0 0 1 1 ? 2020)"}),
    );
    assert_eq!(w.core.next_due(), None, "its last year has passed");
}

#[test]
fn a_service_that_was_reaped_still_fires_the_slot_it_was_woken_for() {
    let mut w = World::new();
    scheduled(&mut w, "rate(5 minutes)");
    let due = w.core.next_due().unwrap();
    // The actor closed and opened again: it remembers nothing it had worked out.
    w.core.due.clear();
    w.clock.set(due);
    w.tick(due);
    assert_eq!(w.drain("q").len(), 1);
}

#[test]
fn a_scheduled_event_can_go_on_to_another_bus() {
    let mut w = World::new();
    w.bus("shop");
    w.ok(
        "PutRule",
        &json!({"Name": "tick", "ScheduleExpression": "rate(1 minute)"}),
    );
    w.ok("PutTargets", &json!({"Rule": "tick", "Targets": [{"Id": "t", "Arn": "arn:aws:events:us-east-1:000000000000:event-bus/shop"}]}));
    let rule = w.rule("shop", "r", &json!({"source": ["aws.events"]}));
    let q = w.queue("q", &rule);
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": q}]}),
    );
    w.core.next_due();
    w.wake_at(START + MINUTE);
    assert_eq!(w.bodies("q")[0]["detail-type"], "Scheduled Event");
}

/// A clock that follows tokio's own, so a test that advances paused time moves it too.
#[derive(Debug)]
struct Paused {
    base: tokio::time::Instant,
}

impl Clock for Paused {
    fn now_ms(&self) -> i64 {
        START + i64::try_from(self.base.elapsed().as_millis()).unwrap()
    }
}

/// EventBridge and SQS on the bus, on paused time, with the timer running as it does in the binary.
struct Rig {
    svc: Arc<Eventbridge>,
    peers: Arc<Peers>,
    h: Harness<Eventbridge>,
    queue: Value,
    _dir: aws_testkit::TempDir,
}

impl Rig {
    async fn new() -> Self {
        let dir = aws_testkit::TempDir::new("eventbridge-timer");
        let peers = Peers::new();
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        let tasks = Tasks::new();
        let ctx = |name: &str| {
            Arc::new(Ctx {
                dir: dir.path().join(name),
                idle: Duration::from_secs(86_400),
                tasks: tasks.clone(),
                clock: Arc::new(Paused {
                    base: tokio::time::Instant::now(),
                }),
                ext: Some(ext.clone()),
            })
        };
        peers.register(Arc::new(aws_sqs::Sqs::new(ctx("sqs"))));
        let svc = Arc::new(Eventbridge::new(ctx("eventbridge")));
        peers.register(svc.clone());
        let h = Harness::new(Eventbridge::new(ctx("unused")));
        let queue = peers
            .call(
                "t",
                &caller(),
                "sqs",
                "CreateQueue",
                &json!({"QueueName": "q"}),
            )
            .await
            .unwrap();
        Self {
            svc,
            peers,
            h,
            queue,
            _dir: dir,
        }
    }

    async fn ask(&self, op: &str, req: Value) -> Value {
        let ix = self.h.op(op);
        let out = self.svc.call(self.h.call(ix, &req).unwrap()).await.unwrap();
        self.h.json(ix, &out)
    }

    async fn queue_arn(&self) -> String {
        let attrs = json!({"QueueUrl": self.queue["QueueUrl"], "AttributeNames": ["QueueArn"]});
        let got = self
            .peers
            .call("t", &caller(), "sqs", "GetQueueAttributes", &attrs)
            .await
            .unwrap();
        got["Attributes"]["QueueArn"].as_str().unwrap().to_owned()
    }

    /// The messages that have arrived since last asked, taken off the queue.
    async fn received(&self) -> usize {
        let url = &self.queue["QueueUrl"];
        let ask = json!({"QueueUrl": url, "MaxNumberOfMessages": 10});
        let got = self
            .peers
            .call("t", &caller(), "sqs", "ReceiveMessage", &ask)
            .await
            .unwrap();
        let messages = got["Messages"].as_array().cloned().unwrap_or_default();
        for m in &messages {
            let gone = json!({"QueueUrl": url, "ReceiptHandle": m["ReceiptHandle"]});
            self.peers
                .call("t", &caller(), "sqs", "DeleteMessage", &gone)
                .await
                .unwrap();
        }
        messages.len()
    }

    /// Waits for the timer task to end, if it is going to.
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

#[tokio::test(start_paused = true)]
async fn the_timer_sleeps_to_the_next_slot_and_is_gone_when_nothing_is_scheduled() {
    let rig = Rig::new().await;
    let rule = json!({"Name": "tick", "ScheduleExpression": "rate(1 minute)"});
    rig.ask("PutRule", rule).await;
    let target = json!({"Id": "t", "Arn": rig.queue_arn().await});
    rig.ask("PutTargets", json!({"Rule": "tick", "Targets": [target]}))
        .await;
    assert!(rig.svc.timer_armed("us-east-1"), "a schedule has a timer");
    assert_eq!(rig.received().await, 0);
    tokio::time::sleep(Duration::from_secs(61)).await;
    // (While the test waits on the disk, tokio's paused clock may run on to the next deadline, so the counts are at least, not exactly.)
    let first = rig.received().await;
    assert!(first >= 1, "the first slot has fired");
    tokio::time::sleep(Duration::from_secs(60)).await;
    let second = rig.received().await;
    assert!(second >= 1, "and again a minute later");
    assert!(first + second <= 6, "one a minute, not a loop");
    rig.ask("RemoveTargets", json!({"Rule": "tick", "Ids": ["t"]}))
        .await;
    assert!(rig.timer_gone().await, "nothing scheduled, no timer");
}
