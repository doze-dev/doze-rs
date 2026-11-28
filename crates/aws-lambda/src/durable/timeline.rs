//! The actor's side of executions on a clock the test moves: a checkpoint's answer, a callback that times out, the execution
//! timeout, retention, and what survives a restart.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_testkit::TempDir;
use aws_wire::service::Caller;
use doze_runtime::ManualClock;
use serde_json::{Value, json};

use super::life::{DurableAsk, DurableReply, Finish, Outcome, Start};
use crate::core::Core;
use crate::req::Req;

const T0: i64 = 1_800_000_000_000;

fn caller() -> Caller {
    Caller {
        access_key_id: "test".into(),
        account_id: "111122223333".into(),
        region: "us-east-1".into(),
        endpoint: "http://127.0.0.1:4566".into(),
        service: None,
        session_token: None,
    }
}

fn req(body: Value) -> Req {
    Req {
        body,
        caller: caller(),
        payload: None,
    }
}

fn open(dir: &TempDir, clock: &Arc<ManualClock>) -> Core {
    let clock: Arc<dyn doze_runtime::Clock> = clock.clone();
    Core::open_in(dir.path(), clock, None).unwrap()
}

fn start(core: &mut Core) -> String {
    let s = Start {
        function: "f".into(),
        version: "$LATEST".into(),
        name: Some("run".into()),
        input: "{}".into(),
        sync: false,
        account: "111122223333".into(),
        endpoint: String::new(),
        scale: 1.0,
        config: json!({"ExecutionTimeout": 10, "RetentionPeriodInDays": 1}),
    };
    let done = core.durable_ask(DurableAsk::Start(Box::new(s)));
    let DurableReply::Started(Ok((arn, true))) = done.reply else {
        panic!("not started")
    };
    assert_eq!(done.wake.runs, vec![arn.clone()]);
    arn
}

/// Begins an invocation and answers its payload.
fn begin(core: &mut Core, arn: &str) -> Value {
    let done = core.durable_ask(DurableAsk::Begin(arn.to_owned()));
    let DurableReply::Begun(Some(b)) = done.reply else {
        panic!("no invocation")
    };
    serde_json::from_str(&b.payload).unwrap()
}

fn finish(core: &mut Core, arn: &str, status: &str) -> super::Wake {
    let f = Finish {
        execution: arn.to_owned(),
        request_id: "r".into(),
        outcome: Outcome::Answered {
            status: status.into(),
            result: None,
            error: None,
        },
    };
    core.durable_ask(DurableAsk::Finish(Box::new(f))).wake
}

fn status(core: &Core, arn: &str) -> String {
    core.execution(arn).unwrap().status.clone()
}

#[tokio::test]
async fn a_callback_times_out_then_the_execution_and_its_retention_end() {
    let dir = TempDir::new("durable");
    let clock = Arc::new(ManualClock::at(T0));
    let mut core = open(&dir, &clock);
    let arn = start(&mut core);
    let payload = begin(&mut core, &arn);
    assert_eq!(
        payload["InitialExecutionState"]["Operations"][0]["Type"],
        "EXECUTION"
    );
    assert_eq!(
        payload["InitialExecutionState"]["Operations"][0]["StartTimestamp"],
        "2027-01-15T08:00:00.000Z"
    );
    let token = payload["CheckpointToken"].as_str().unwrap().to_owned();
    // A second invocation does not begin while one runs.
    assert!(matches!(
        core.durable_ask(DurableAsk::Begin(arn.clone())).reply,
        DurableReply::Begun(None)
    ));

    let updates = json!([{"Id": "cb", "Type": "CALLBACK", "Action": "START", "CallbackOptions": {"TimeoutSeconds": 3}}]);
    let out = core
        .checkpoint_durable(&req(
            json!({"DurableExecutionArn": arn, "CheckpointToken": token, "Updates": updates}),
        ))
        .unwrap()
        .json;
    let op = &out["NewExecutionState"]["Operations"][0];
    assert_eq!(op["Status"], "STARTED");
    assert!(op["CallbackDetails"]["CallbackId"].as_str().is_some());
    let stale = core.checkpoint_durable(&req(
        json!({"DurableExecutionArn": arn, "CheckpointToken": token}),
    ));
    assert_eq!(stale.unwrap_err().code, "InvalidParameterValueException");
    assert!(
        finish(&mut core, &arn, "PENDING").runs.is_empty(),
        "nothing new to see"
    );
    assert_eq!(core.durable_due(), Some(T0 + 3000));

    clock.advance(3000);
    let wake = core.durable_ask(DurableAsk::Tick).wake;
    assert_eq!(
        wake.runs,
        vec![arn.clone()],
        "the timeout wakes the execution"
    );
    let payload = begin(&mut core, &arn);
    assert_eq!(payload["UpdatedOperationIds"], json!(["cb"]));
    assert_eq!(
        payload["InitialExecutionState"]["Operations"][1]["Status"],
        "TIMED_OUT"
    );
    finish(&mut core, &arn, "PENDING");
    assert_eq!(
        status(&core, &arn),
        "FAILED",
        "PENDING with nothing to wait for fails"
    );

    clock.advance(86_400_000);
    core.durable_ask(DurableAsk::Tick);
    assert!(core.execution(&arn).is_err(), "gone after its retention");
}

#[tokio::test]
async fn the_execution_timeout_ends_a_wait_and_a_restart_keeps_the_state() {
    let dir = TempDir::new("durable");
    let clock = Arc::new(ManualClock::at(T0));
    let arn = {
        let mut core = open(&dir, &clock);
        let arn = start(&mut core);
        let token = begin(&mut core, &arn)["CheckpointToken"]
            .as_str()
            .unwrap()
            .to_owned();
        let updates = json!([
            {"Id": "s", "Type": "STEP", "Action": "START"},
            {"Id": "s", "Type": "STEP", "Action": "SUCCEED", "Payload": "2"},
            {"Id": "w", "Type": "WAIT", "Action": "START", "WaitOptions": {"WaitSeconds": 60}}
        ]);
        let body =
            json!({"DurableExecutionArn": arn, "CheckpointToken": token, "Updates": updates});
        core.checkpoint_durable(&req(body)).unwrap();
        // The process stops mid-invocation.
        if let Ok(Some(ack)) = core.flush() {
            ack.await.unwrap();
        }
        arn
    };
    let mut core = open(&dir, &clock);
    let wake = core.durable_ask(DurableAsk::Tick).wake;
    assert_eq!(
        wake.runs,
        vec![arn.clone()],
        "what was running runs again from its checkpoints"
    );
    let payload = begin(&mut core, &arn);
    let ops = payload["InitialExecutionState"]["Operations"]
        .as_array()
        .unwrap();
    assert_eq!(ops.len(), 3);
    assert_eq!(ops[1]["StepDetails"]["Result"], "2");
    finish(&mut core, &arn, "PENDING");
    assert_eq!(
        core.durable_due(),
        Some(T0 + 10_000),
        "the execution timeout comes before the wait"
    );
    clock.advance(10_000);
    let wake = core.durable_ask(DurableAsk::Tick).wake;
    assert_eq!(wake.closed, vec![arn.clone()]);
    assert_eq!(status(&core, &arn), "TIMED_OUT");
    let history = core
        .get_durable_history(&req(json!({"DurableExecutionArn": arn})))
        .unwrap()
        .json;
    let last = history["Events"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["EventType"], "ExecutionTimedOut");
}
