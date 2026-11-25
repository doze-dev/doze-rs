//! Forced events (the console's Lab): a schedule is invoked now, by the path its timer takes.

use std::sync::Arc;
use std::time::Duration;

use aws_peers::{Authorizer, Mode, Need, Peers};
use aws_testkit::{Harness, TempDir};
use aws_wire::ApiError;
use aws_wire::forced::Forced as _;
use aws_wire::forced::{Done, Step, kind};
use aws_wire::service::{BoxFuture, Caller, Service};
use serde_json::{Value, json};

use super::{ACCOUNT, START, World, caller, ctx, schedule};
use crate::Scheduler;
use crate::fire::NOTHING;
use crate::model::key;

const QUEUE: &str = "arn:aws:sqs:us-east-1:000000000000:q";
const HOUR: i64 = 3_600_000;

fn next_ms(w: &World, name: &str) -> Option<i64> {
    w.core
        .schedules
        .get(&key(ACCOUNT, &["default", name]))
        .and_then(|s| s.next_ms)
}

fn done(step: Step) -> Done {
    match step {
        Step::Done(d) => d,
        Step::Nothing(why) => panic!("it did nothing: {why}"),
    }
}

/// A scheduler on a real directory with SQS beside it on the bus, and the harness that builds its calls.
struct Stack {
    scheduler: Scheduler,
    peers: Arc<Peers>,
    h: Harness<Scheduler>,
    _dir: TempDir,
}

impl Stack {
    fn new(peers: Arc<Peers>, dir: TempDir) -> Self {
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        peers.register(Arc::new(aws_sqs::Sqs::new(ctx(
            &dir.path().join("sqs"),
            Some(ext.clone()),
        ))));
        Self {
            scheduler: Scheduler::new(ctx(&dir.path().join("scheduler"), Some(ext))),
            peers,
            h: Harness::new(Scheduler::new(ctx(&std::env::temp_dir(), None))),
            _dir: dir,
        }
    }

    fn open() -> Self {
        let peers = Peers::new();
        peers.set_mode(Mode::Off);
        Self::new(peers, TempDir::new("scheduler-forced"))
    }

    async fn create(&self, request: &Value) {
        let call = self.h.call(self.h.op("CreateSchedule"), request).unwrap();
        self.scheduler.call(call).await.unwrap();
    }

    async fn bus(&self, op: &str, body: Value) -> Value {
        self.peers
            .call("test", &caller(), "sqs", op, &body)
            .await
            .unwrap()
    }

    async fn queue(&self, name: &str) -> String {
        let made = self.bus("CreateQueue", json!({"QueueName": name})).await;
        made["QueueUrl"].as_str().unwrap().to_owned()
    }

    async fn messages(&self, url: &str) -> Vec<Value> {
        let got = self
            .bus(
                "ReceiveMessage",
                json!({"QueueUrl": url, "MaxNumberOfMessages": 10}),
            )
            .await;
        got.get("Messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    async fn fire(&self, who: &Caller, group: &str, name: &str) -> Result<Step, ApiError> {
        let input = json!({"group": group, "name": name});
        self.scheduler
            .forced()
            .unwrap()
            .force(who, kind::FIRE_SCHEDULE, &input)
            .await
    }
}

#[tokio::test]
async fn a_schedule_due_in_a_day_fires_now_and_its_target_gets_the_message_exactly_once() {
    let s = Stack::open();
    let url = s.queue("email-outbox").await;
    let mut req = schedule(
        "nightly-report",
        "cron(0 3 * * ? 2099)",
        "arn:aws:sqs:us-east-1:000000000000:email-outbox",
    );
    req["Target"]["Input"] = json!("the report");
    s.create(&req).await;
    assert!(s.messages(&url).await.is_empty(), "it is not due");

    let d = done(
        s.fire(&caller(), "default", "nightly-report")
            .await
            .unwrap(),
    );
    assert_eq!(d.subject, "default/nightly-report");
    assert!(
        d.sentence
            .starts_with("Fired nightly-report now: it sent a message to email-outbox."),
        "{}",
        d.sentence
    );
    assert!(
        d.sentence
            .ends_with("Its next scheduled fire is unchanged: 2099-01-01 03:00 UTC."),
        "{}",
        d.sentence
    );
    let got = s.messages(&url).await;
    assert_eq!(got.len(), 1, "exactly once");
    assert_eq!(got[0]["Body"], "the report");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(s.messages(&url).await.is_empty(), "and only once");
}

#[tokio::test]
async fn a_target_that_refuses_is_said_with_its_code_and_what_the_retry_policy_does() {
    let s = Stack::open();
    // The queue is not there: the target refuses it as wrong, which a scheduled invocation does not retry either.
    s.create(&schedule("gone", "rate(1 day)", QUEUE)).await;
    let d = done(s.fire(&caller(), "default", "gone").await.unwrap());
    assert!(
        d.sentence
            .starts_with("Fired gone now: the target q (an SQS queue) refused it with"),
        "{}",
        d.sentence
    );
    assert!(d.sentence.contains("That is not retried"), "{}", d.sentence);
}

#[test]
fn a_recurring_schedule_keeps_its_plan_and_its_timer_still_fires_it_when_due() {
    let mut w = World::new();
    w.ok("CreateSchedule", &schedule("every", "rate(1 day)", QUEUE));
    assert_eq!(w.tick().len(), 1, "its first fire is at once");
    let due = START + 24 * HOUR;
    assert_eq!(next_ms(&w, "every"), Some(due));

    w.clock.advance(5 * HOUR);
    let left = w.core.fire_checked(ACCOUNT, "default", "every").unwrap();
    assert_eq!(left["next_ms"], due);
    let fired = std::mem::take(&mut w.core.fired);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].scheduled_ms, START + 5 * HOUR, "invoked now");
    assert_eq!(next_ms(&w, "every"), Some(due), "the plan is the same");
    assert!(w.tick().is_empty(), "nothing else was fired");

    w.clock.set(due);
    let fired = w.tick();
    assert_eq!(
        fired.len(),
        1,
        "the timer fires it when it is due, as it would have"
    );
    assert_eq!(fired[0].scheduled_ms, due);
}

#[test]
fn a_one_time_schedule_has_one_fire_and_it_is_the_forced_one() {
    let mut w = World::new();
    let mut keep = schedule("keep", "at(2099-06-01T00:00:00)", QUEUE);
    keep["ActionAfterCompletion"] = json!("NONE");
    w.ok("CreateSchedule", &keep);
    let mut drop = schedule("drop", "at(2099-06-01T00:00:00)", QUEUE);
    drop["ActionAfterCompletion"] = json!("DELETE");
    w.ok("CreateSchedule", &drop);

    let left = w.core.fire_checked(ACCOUNT, "default", "keep").unwrap();
    assert_eq!(
        (left["once"].clone(), left["removed"].clone()),
        (json!(true), json!(false))
    );
    let left = w.core.fire_checked(ACCOUNT, "default", "drop").unwrap();
    assert_eq!(left["removed"], json!(true));
    assert_eq!(std::mem::take(&mut w.core.fired).len(), 2);

    // The kept one has no fire left: its timer is not running, so the forced one is refused with the words, and nothing is fired.
    let e = w.core.fire_checked(ACCOUNT, "default", "keep").unwrap_err();
    assert_eq!(e.code, NOTHING);
    assert!(
        e.message.contains(
            "keep has no fire left: it is a one-time schedule and its one fire has already happened"
        ),
        "{}",
        e.message
    );
    assert!(w.core.fired.is_empty());
    // The deleted one is not there.
    let e = w.core.fire_checked(ACCOUNT, "default", "drop").unwrap_err();
    assert_eq!(e.code, "ResourceNotFoundException");
}

#[test]
fn a_disabled_schedule_has_nothing_to_fire_and_an_ended_one_says_so() {
    let mut w = World::new();
    let mut off = schedule("off", "rate(1 day)", QUEUE);
    off["State"] = json!("DISABLED");
    w.ok("CreateSchedule", &off);
    let e = w.core.fire_checked(ACCOUNT, "default", "off").unwrap_err();
    assert_eq!(e.code, NOTHING);
    assert!(
        e.message
            .starts_with("off is disabled: its timer holds no fire"),
        "{}",
        e.message
    );

    let mut ended = schedule("ended", "rate(1 hour)", QUEUE);
    ended["StartDate"] = json!(START / 1000);
    ended["EndDate"] = json!((START + 90 * 60_000) / 1000);
    w.ok("CreateSchedule", &ended);
    w.clock.advance(3 * HOUR);
    w.tick();
    let e = w
        .core
        .fire_checked(ACCOUNT, "default", "ended")
        .unwrap_err();
    assert_eq!(e.code, NOTHING);
    assert!(
        e.message.contains("its EndDate has passed"),
        "{}",
        e.message
    );
    assert!(w.core.fired.is_empty(), "nothing was invoked");
}

struct Refuses(&'static str);

impl Authorizer for Refuses {
    fn authorize<'a>(
        &'a self,
        caller: &'a Caller,
        needs: Vec<Need>,
    ) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(async move {
            if caller.access_key_id == self.0 {
                let what = needs
                    .first()
                    .map(|n| format!("{} on {}", n.action, n.resource));
                return Err(ApiError::new(
                    403,
                    "AccessDeniedException",
                    what.unwrap_or_default(),
                ));
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn a_caller_without_the_right_is_refused_and_nothing_is_fired() {
    let peers = Peers::new();
    peers.set_authorizer(Arc::new(Refuses("intruder")));
    peers.set_mode(Mode::Enforce);
    let s = Stack::new(peers, TempDir::new("scheduler-forced-denied"));
    let url = s.queue("q").await;
    s.create(&schedule("s", "cron(0 3 * * ? 2099)", QUEUE))
        .await;
    let mut intruder = caller();
    intruder.access_key_id = "intruder".into();
    let e = s.fire(&intruder, "default", "s").await.unwrap_err();
    assert_eq!((e.status, e.code.as_ref()), (403, "AccessDeniedException"));
    assert!(e.message.contains(
        "scheduler:UpdateSchedule on arn:aws:scheduler:us-east-1:000000000000:schedule/default/s"
    ));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(s.messages(&url).await.is_empty(), "nothing was fired");
    // The right caller's fire is carried out (whether the role can deliver is IAM's question, not asked here).
    assert!(matches!(
        s.fire(&caller(), "default", "s").await.unwrap(),
        Step::Done(_)
    ));
}

#[tokio::test]
async fn a_missing_schedule_a_missing_name_and_another_kind_are_refused_in_words() {
    let s = Stack::open();
    let e = s.fire(&caller(), "default", "nope").await.unwrap_err();
    assert_eq!(e.code, "ResourceNotFoundException");
    let forced = s.scheduler.forced().unwrap();
    let e = forced
        .force(&caller(), kind::FIRE_SCHEDULE, &json!({}))
        .await
        .unwrap_err();
    assert_eq!(e.message, "The forced event needs a schedule.");
    let e = forced
        .force(&caller(), kind::SWEEP_TTL, &json!({"name": "x"}))
        .await
        .unwrap_err();
    assert!(
        e.message
            .contains("Scheduler has no forced event sweep-ttl"),
        "{}",
        e.message
    );
}
