//! The interpreter driven without a service: the recorded machines, retries and catches, waits, tokens, fan-outs.

use serde_json::{Value, json};

use super::{Dispatch, End, Exec, Outcome, Start, Status, Step};
use crate::def::{Definition, parse};
use crate::env::Env;

fn def(v: &Value) -> Definition {
    parse(&v.to_string()).unwrap()
}

fn start(d: &Definition, input: Value) -> (Exec, Step) {
    let (mut ex, first) = Exec::start(
        d,
        Start {
            arn: "arn:aws:states:us-east-1:1:execution:m:e".into(),
            name: "e".into(),
            machine_arn: "arn:aws:states:us-east-1:1:stateMachine:m".into(),
            machine_name: "m".into(),
            role_arn: "arn:aws:iam::1:role/r".into(),
            input: input.to_string(),
            started_extra: Vec::new(),
        },
        1_000,
    );
    let mut step = ex.run(d, &Env::new(1_000, 9));
    step.events.insert(0, first);
    (ex, step)
}

fn output(step: &Step) -> Value {
    match &step.end {
        Some(End::Succeeded(o)) => serde_json::from_str(o).unwrap(),
        other => panic!("not succeeded: {other:?}"),
    }
}

fn run(machine: Value, input: Value) -> Step {
    let d = def(&machine);
    start(&d, input).1
}

#[test]
fn the_recorded_history_shape() {
    let step = run(
        json!({"StartAt": "Greet", "States": {"Greet": {"Type": "Pass", "Parameters": {"hello.$": "$.who"}, "End": true}}}),
        json!({"who": "world"}),
    );
    let kinds: Vec<&str> = step.events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "ExecutionStarted",
            "PassStateEntered",
            "PassStateExited",
            "ExecutionSucceeded"
        ]
    );
    let links: Vec<(u64, u64)> = step.events.iter().map(|e| (e.id, e.prev)).collect();
    assert_eq!(links, [(1, 0), (2, 0), (3, 2), (4, 3)]);
    assert_eq!(output(&step), json!({"hello": "world"}));
}

#[test]
fn the_recorded_language_cases() {
    let paths = run(
        json!({"StartAt": "Shape", "States": {"Shape": {"Type": "Pass", "InputPath": "$.order",
            "Parameters": {"id.$": "$.id", "first.$": "$.items[0]", "count.$": "States.ArrayLength($.items)"},
            "ResultPath": "$.summary", "OutputPath": "$.summary", "End": true}}}),
        json!({"order": {"id": 7, "items": ["book", "pen"]}, "noise": true}),
    );
    assert_eq!(
        output(&paths),
        json!({"count": 2, "first": "book", "id": 7})
    );
    let merged = run(
        json!({"StartAt": "S", "States": {"S": {"Type": "Pass", "Result": {"ok": true}, "ResultPath": "$.check", "End": true}}}),
        json!({"kept": 1}),
    );
    assert_eq!(output(&merged), json!({"kept": 1, "check": {"ok": true}}));
    let failed = run(
        json!({"StartAt": "Reach", "States": {"Reach": {"Type": "Pass", "Parameters": {"x.$": "$.not.there"}, "End": true}}}),
        json!({}),
    );
    assert_eq!(
        failed.end,
        Some(End::Failed(crate::Failure::new(
            "States.Runtime",
            "An error occurred while executing the state 'Reach' (entered at the event id #2). The JSONPath '$.not.there' specified for the field 'x.$' could not be found in the input '{}'"
        )))
    );
    let stop = run(
        json!({"StartAt": "Stop", "States": {"Stop": {"Type": "Fail", "Error": "Order.Invalid", "Cause": "no items"}}}),
        json!({}),
    );
    assert_eq!(
        stop.end,
        Some(End::Failed(crate::Failure::new(
            "Order.Invalid",
            "no items"
        )))
    );
}

#[test]
fn choice_map_and_parallel() {
    let choice = json!({"StartAt": "Size", "States": {
        "Size": {"Type": "Choice", "Choices": [
            {"And": [{"Variable": "$.total", "NumericGreaterThanEquals": 100}, {"Variable": "$.tier", "StringEquals": "gold"}], "Next": "Big"},
            {"Variable": "$.total", "NumericLessThan": 10, "Next": "Small"},
            {"Not": {"Variable": "$.coupon", "IsPresent": true}, "Next": "Plain"}],
            "Default": "Other"},
        "Big": {"Type": "Pass", "Result": "big", "End": true},
        "Small": {"Type": "Pass", "Result": "small", "End": true},
        "Plain": {"Type": "Pass", "Result": "plain", "End": true},
        "Other": {"Type": "Pass", "Result": "other", "End": true}}});
    assert_eq!(
        output(&run(choice.clone(), json!({"total": 250, "tier": "gold"}))),
        json!("big")
    );
    assert_eq!(
        output(&run(choice.clone(), json!({"total": 5}))),
        json!("small")
    );
    assert_eq!(
        output(&run(choice, json!({"total": 50, "coupon": 1}))),
        json!("other")
    );
    let map = run(
        json!({"StartAt": "Each", "States": {"Each": {"Type": "Map", "ItemsPath": "$.items", "MaxConcurrency": 1,
            "ItemSelector": {"value.$": "$$.Map.Item.Value", "index.$": "$$.Map.Item.Index"},
            "ItemProcessor": {"ProcessorConfig": {"Mode": "INLINE"}, "StartAt": "Tag", "States": {
                "Tag": {"Type": "Pass", "Parameters": {"seen.$": "$.value", "at.$": "$.index"}, "End": true}}},
            "End": true}}}),
        json!({"items": ["a", "b", "c"]}),
    );
    assert_eq!(
        output(&map),
        json!([{"at": 0, "seen": "a"}, {"at": 1, "seen": "b"}, {"at": 2, "seen": "c"}])
    );
    let parallel = run(
        json!({"StartAt": "Both", "States": {"Both": {"Type": "Parallel", "End": true, "Branches": [
            {"StartAt": "L", "States": {"L": {"Type": "Pass", "Result": "left", "End": true}}},
            {"StartAt": "R", "States": {"R": {"Type": "Pass", "Parameters": {"got.$": "$.v"}, "End": true}}}]}}}),
        json!({"v": 1}),
    );
    assert_eq!(output(&parallel), json!(["left", {"got": 1}]));
}

fn task_machine() -> Definition {
    def(&json!({"StartAt": "Call", "States": {
        "Call": {"Type": "Task", "Resource": "arn:aws:lambda:us-east-1:1:function:f", "TimeoutSeconds": 5,
            "Retry": [{"ErrorEquals": ["Flaky"], "IntervalSeconds": 2, "MaxAttempts": 1}],
            "Catch": [{"ErrorEquals": ["States.ALL"], "ResultPath": "$.err", "Next": "Caught"}],
            "ResultPath": "$.out", "End": true},
        "Caught": {"Type": "Pass", "End": true}}}))
}

fn only_call(step: &Step) -> Dispatch {
    assert_eq!(step.dispatches.len(), 1, "{step:?}");
    step.dispatches[0].clone()
}

#[test]
fn a_task_succeeds_after_a_retry() {
    let d = task_machine();
    let (mut ex, step) = start(&d, json!({"a": 1}));
    let call = only_call(&step);
    assert_eq!(call.input, json!({"a": 1}));
    let failed = ex.deliver(
        &d,
        call.frame,
        call.seq,
        Outcome::Failed(crate::Failure::new("Flaky", "")),
        &Env::new(1_100, 1),
    );
    assert!(failed.dispatches.is_empty());
    assert_eq!(ex.next_due(), Some(3_100));
    let again = ex.wake(&d, &Env::new(3_100, 1));
    let call2 = only_call(&again);
    assert_eq!(call2.seq, call.seq + 1);
    // The answer to the first call is stale and ignored.
    assert!(
        ex.deliver(
            &d,
            call.frame,
            call.seq,
            Outcome::Done(json!(0)),
            &Env::new(3_200, 1)
        )
        .end
        .is_none()
    );
    let done = ex.deliver(
        &d,
        call2.frame,
        call2.seq,
        Outcome::Done(json!({"r": 2})),
        &Env::new(3_300, 1),
    );
    assert_eq!(output(&done), json!({"a": 1, "out": {"r": 2}}));
}

#[test]
fn a_timeout_is_caught() {
    let d = task_machine();
    let (mut ex, _) = start(&d, json!({}));
    assert_eq!(ex.next_due(), Some(6_000));
    let step = ex.wake(&d, &Env::new(6_000, 1));
    let kinds: Vec<&str> = step.events.iter().map(|e| e.kind.as_str()).collect();
    assert!(kinds.contains(&"LambdaFunctionTimedOut"), "{kinds:?}");
    assert_eq!(
        output(&step),
        json!({"err": {"Error": "States.Timeout", "Cause": ""}})
    );
}

#[test]
fn waits_sleep_to_their_deadline() {
    let d = def(
        &json!({"StartAt": "W", "States": {"W": {"Type": "Wait", "Seconds": 10, "Next": "D"}, "D": {"Type": "Succeed"}}}),
    );
    let (mut ex, step) = start(&d, json!({"x": 1}));
    assert!(step.end.is_none());
    assert_eq!(ex.frame(1).unwrap().status, Status::Sleeping);
    assert_eq!(ex.next_due(), Some(11_000));
    assert!(ex.wake(&d, &Env::new(10_999, 1)).end.is_none());
    assert_eq!(output(&ex.wake(&d, &Env::new(11_000, 1))), json!({"x": 1}));
    assert_eq!(ex.next_due(), None);
}

#[test]
fn a_token_parks_until_it_is_answered() {
    let d = def(&json!({"StartAt": "T", "States": {"T": {"Type": "Task",
        "Resource": "arn:aws:states:::sqs:sendMessage.waitForTaskToken",
        "Parameters": {"QueueUrl": "q", "MessageBody": {"token.$": "$$.Task.Token"}}, "End": true}}}));
    let (mut ex, step) = start(&d, json!({}));
    let call = only_call(&step);
    let token = call.token.clone().unwrap();
    assert_eq!(call.input["MessageBody"]["token"], json!(token));
    let sub = ex.deliver(
        &d,
        call.frame,
        call.seq,
        Outcome::Submitted(json!({"MessageId": "m"})),
        &Env::new(1_100, 1),
    );
    assert_eq!(sub.events[0].kind, "TaskSubmitted");
    let frame = ex.by_token(&token).unwrap().id;
    let seq = ex.frame(frame).unwrap().seq;
    let done = ex.deliver(
        &d,
        frame,
        seq,
        Outcome::Done(json!({"ok": 1})),
        &Env::new(1_200, 1),
    );
    assert_eq!(output(&done), json!({"ok": 1}));
}

#[test]
#[cfg(feature = "jsonata")]
fn jsonata_states_run() {
    let step = run(
        json!({"QueryLanguage": "JSONata", "StartAt": "A", "States": {
            "A": {"Type": "Pass", "Assign": {"n": "{% $states.input.n * 2 %}"}, "Output": {"seen": "{% $states.input.n %}"}, "Next": "B"},
            "B": {"Type": "Choice", "Choices": [{"Condition": "{% $n > 5 %}", "Next": "Big"}], "Default": "Small"},
            "Big": {"Type": "Pass", "Output": "{% $n %}", "End": true},
            "Small": {"Type": "Pass", "Output": "small", "End": true}}}),
        json!({"n": 3}),
    );
    assert_eq!(output(&step), json!(6));
}

#[test]
fn a_failed_branch_fails_the_parallel_and_a_catch_takes_it() {
    let step = run(
        json!({"StartAt": "P", "States": {
            "P": {"Type": "Parallel", "Branches": [
                {"StartAt": "Ok", "States": {"Ok": {"Type": "Pass", "End": true}}},
                {"StartAt": "No", "States": {"No": {"Type": "Fail", "Error": "Boom", "Cause": "c"}}}],
              "Catch": [{"ErrorEquals": ["Boom"], "Next": "After"}], "End": true},
            "After": {"Type": "Pass", "End": true}}}),
        json!({}),
    );
    assert_eq!(output(&step), json!({"Error": "Boom", "Cause": "c"}));
}

#[test]
fn a_runtime_error_is_not_caught() {
    let step = run(
        json!({"StartAt": "T", "States": {"T": {"Type": "Task", "Resource": "arn:aws:states:::lambda:invoke",
            "Parameters": {"x.$": "$.nope"}, "Catch": [{"ErrorEquals": ["States.ALL"], "Next": "C"}], "End": true},
            "C": {"Type": "Succeed"}}}),
        json!({}),
    );
    assert!(matches!(step.end, Some(End::Failed(ref f)) if f.error == "States.Runtime"));
}
