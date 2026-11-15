use serde_json::{Value, json};

use super::{World, schedule};
use crate::deliver::{Failure, Outcome, attempt};
use crate::fire::Invocation;

fn fire(w: &mut World, req: &Value) -> Invocation {
    w.ok("CreateSchedule", req);
    let mut fired = w.tick();
    assert_eq!(fired.len(), 1);
    fired.remove(0)
}

#[test]
fn an_sqs_target_gets_its_input_with_the_context_attributes() {
    let mut w = World::new();
    let (url, arn) = w.queue("q", json!({}));
    let mut s = schedule("s", "rate(1 hour)", &arn);
    s["Target"]["Input"] = json!(
        "<aws.scheduler.schedule-arn> at <aws.scheduler.scheduled-time> try <aws.scheduler.attempt-number>"
    );
    let inv = fire(&mut w, &s);
    let sh = w.shared();
    assert!(matches!(
        w.rt.block_on(attempt(&sh, &inv, 1)),
        Outcome::Sent
    ));
    let got = w.receive(&url);
    assert_eq!(
        got[0]["Body"],
        "arn:aws:scheduler:us-east-1:000000000000:schedule/default/s at 2026-01-01T00:00:00Z try 1"
    );
}

#[test]
fn a_fifo_queue_gets_the_message_group() {
    let mut w = World::new();
    let (url, arn) = w.queue("q.fifo", json!({"FifoQueue": "true"}));
    let mut s = schedule("s", "rate(1 hour)", &arn);
    s["Target"]["SqsParameters"] = json!({"MessageGroupId": "g1"});
    let inv = fire(&mut w, &s);
    let sh = w.shared();
    assert!(matches!(
        w.rt.block_on(attempt(&sh, &inv, 1)),
        Outcome::Sent
    ));
    assert_eq!(w.receive(&url)[0]["Body"], "hello");
}

#[test]
fn a_universal_target_calls_the_named_action_with_the_input() {
    let mut w = World::new();
    let (url, _) = w.queue("u", json!({}));
    let mut s = schedule(
        "s",
        "rate(1 hour)",
        "arn:aws:scheduler:::aws-sdk:sqs:sendMessage",
    );
    s["Target"]["Input"] = json!(json!({"QueueUrl": url, "MessageBody": "via sdk"}).to_string());
    let inv = fire(&mut w, &s);
    let sh = w.shared();
    assert!(matches!(
        w.rt.block_on(attempt(&sh, &inv, 1)),
        Outcome::Sent
    ));
    assert_eq!(w.receive(&url)[0]["Body"], "via sdk");
}

#[test]
fn a_service_doze_does_not_run_is_said_so_and_not_dead_lettered() {
    let mut w = World::new();
    let ecs = "arn:aws:ecs:us-east-1:000000000000:cluster/c";
    let inv = fire(&mut w, &schedule("s", "rate(1 hour)", ecs));
    let sh = w.shared();
    assert!(matches!(
        w.rt.block_on(attempt(&sh, &inv, 1)),
        Outcome::NotLocal(_)
    ));
    let u = schedule(
        "u",
        "rate(1 hour)",
        "arn:aws:scheduler:::aws-sdk:ecs:runTask",
    );
    let inv = fire(&mut w, &u);
    assert!(matches!(
        w.rt.block_on(attempt(&sh, &inv, 1)),
        Outcome::NotLocal(_)
    ));
}

#[test]
fn a_failure_goes_to_the_dead_letter_queue_with_aws_attributes() {
    let mut w = World::new();
    let (dlq_url, dlq) = w.queue("dlq", json!({}));
    let mut s = schedule(
        "s",
        "rate(1 hour)",
        "arn:aws:sqs:us-east-1:000000000000:missing",
    );
    s["Target"]["DeadLetterConfig"] = json!({"Arn": dlq});
    let inv = fire(&mut w, &s);
    let sh = w.shared();
    let first = w.rt.block_on(attempt(&sh, &inv, 1));
    assert!(matches!(&first, Outcome::Failed(f) if !f.retryable));
    w.rt.block_on(crate::retry::carry_on(sh, inv.clone(), 1, first));
    let got = w.receive(&dlq_url);
    let a = &got[0]["MessageAttributes"];
    assert_eq!(got[0]["Body"], "hello");
    assert_eq!(a["SCHEDULE_ARN"]["StringValue"], inv.schedule_arn);
    assert_eq!(a["EXECUTION_ID"]["StringValue"], inv.execution_id);
    assert_eq!(a["RETRY_ATTEMPTS"]["StringValue"], "0");
    assert_eq!(a["SCHEDULED_TIME"]["StringValue"], "2026-01-01T00:00:00Z");
    assert_eq!(
        a["TARGET_ARN"]["StringValue"],
        "arn:aws:sqs:us-east-1:000000000000:missing"
    );
    assert!(
        a["ERROR_CODE"]["StringValue"]
            .as_str()
            .unwrap()
            .contains("NonExistentQueue"),
        "{a}"
    );
    assert!(
        a.get("EXHAUSTED_RETRY_CONDITION").is_none(),
        "only a retryable failure says which limit it met"
    );
}

#[test]
fn the_retry_policy_decides_when_to_give_up() {
    let mut w = World::new();
    let mut s = schedule("s", "rate(1 hour)", "arn:aws:sqs:us-east-1:000000000000:q");
    s["Target"]["RetryPolicy"] = json!({"MaximumRetryAttempts": 2, "MaximumEventAgeInSeconds": 60});
    let inv = fire(&mut w, &s);
    let busy = Failure {
        code: "ThrottlingException".into(),
        message: "slow down".into(),
        retryable: true,
    };
    let at = inv.scheduled_ms;
    assert_eq!(crate::retry::next(&inv, &busy, 1, at), Ok(1));
    assert_eq!(crate::retry::next(&inv, &busy, 2, at), Ok(2));
    assert_eq!(
        crate::retry::next(&inv, &busy, 3, at),
        Err(Some("MaximumRetryAttempts"))
    );
    assert_eq!(
        crate::retry::next(&inv, &busy, 1, at + 60_000),
        Err(Some("MaximumEventAgeInSeconds"))
    );
    let wrong = Failure {
        retryable: false,
        ..busy
    };
    assert_eq!(crate::retry::next(&inv, &wrong, 1, at), Err(None));
    assert_eq!(crate::retry::backoff_seconds(10), 64);
}

#[test]
fn roles_must_trust_scheduler() {
    let trusted = r#"{"Statement":[{"Effect":"Allow","Principal":{"Service":"scheduler.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
    assert!(crate::role::trusts_scheduler(trusted));
    let other = r#"{"Statement":[{"Effect":"Allow","Principal":{"Service":["lambda.amazonaws.com"]},"Action":"sts:AssumeRole"}]}"#;
    assert!(!crate::role::trusts_scheduler(other));
    assert!(!crate::role::trusts_scheduler("not json"));
}
