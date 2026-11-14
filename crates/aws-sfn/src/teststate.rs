//! `TestState`: one state run on its own, beside the actor (it keeps nothing). The state is put in a machine of its own whose other
//! states are long waits, so the run stops where the state hands over: the next state's name, the output, and whether the state
//! succeeded, failed, caught an error or would retry. A Task calls its service as the given role, or answers from `mock`
//! (`result` or `errorOutput`); a Map with an `ItemReader` reads `stateConfiguration.mapItemReaderData`. `inspectionData` shows
//! the input after each step at `DEBUG` and `TRACE`.

use std::sync::Arc;

use aws_asl::{Definition, Dispatch, Env, Exec, Outcome, Start, Status, Step};
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Map, Value, json};

use crate::effects::Job;
use crate::req::invalid_definition;
use crate::shared::Shared;

/// A state no run gets past.
const PARKED: &str = r#"{"Type": "Wait", "Seconds": 99999999, "End": true}"#;

/// The machine a tested state runs in, and the state's name.
fn machine(r: &Value) -> Result<(String, String), ApiError> {
    let text = r
        .get("definition")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let doc = aws_asl::jackson::read(text).map_err(|e| {
        invalid_definition(format!(
            "Invalid State Machine Definition: 'INVALID_JSON_DESCRIPTION: {}'",
            e.located(text)
        ))
    })?;
    let (state, name, lang) = match r.get("stateName").and_then(Value::as_str) {
        Some(n) => {
            let s = doc
                .pointer(&format!("/States/{n}"))
                .cloned()
                .ok_or_else(|| {
                    crate::req::invalid(format!("The state '{n}' is not in the definition")) // (memory)
                })?;
            (s, n.to_owned(), doc.get("QueryLanguage").cloned())
        }
        None => (doc, "TestState".to_owned(), None),
    };
    let mut states = Map::new();
    let parked: Value = serde_json::from_str(PARKED).unwrap_or(Value::Null);
    for target in targets(&state) {
        if target != name {
            states.insert(target, parked.clone());
        }
    }
    states.insert(name.clone(), state);
    let mut m = json!({"StartAt": name, "States": states});
    if let (Some(l), Some(o)) = (lang, m.as_object_mut()) {
        o.insert("QueryLanguage".into(), l);
    }
    Ok((m.to_string(), name))
}

/// The states a state may go to.
fn targets(s: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |v: Option<&Value>| out.extend(v.and_then(Value::as_str).map(str::to_owned));
    add(s.get("Next"));
    add(s.get("Default"));
    for list in ["Choices", "Catch"] {
        for c in s.get(list).and_then(Value::as_array).into_iter().flatten() {
            add(c.get("Next"));
        }
    }
    out
}

/// What the run met: the result of the tested state's call, and the last failure.
#[derive(Default)]
struct Seen {
    result: Option<Value>,
    failure: Option<(String, String)>,
}

pub(crate) async fn run(
    shared: &Arc<Shared>,
    caller: &Caller,
    r: &Value,
) -> Result<Value, ApiError> {
    let (text, name) = machine(r)?;
    let def = aws_asl::parse(&text).map_err(|d| invalid_definition(aws_asl::def::refusal(&d)))?;
    let input = r
        .get("input")
        .and_then(Value::as_str)
        .unwrap_or("{}")
        .to_owned();
    crate::executions::check_input(&input)?;
    let now = shared.ctx.clock.now_ms();
    let mut env = Env::new(now, u64::try_from(now).unwrap_or(1));
    caller.region.clone_into(&mut env.region);
    caller.account_id.clone_into(&mut env.account);
    let mut ex = start(&def, r, caller, input, now);
    let before = ex.clone();
    let mut seen = Seen::default();
    let mut step = ex.run(&def, &env);
    for _ in 0..64 {
        note(&mut seen, &step);
        let calls = std::mem::take(&mut step.dispatches);
        if calls.is_empty() {
            if let Some(f) = ex
                .frame(1)
                .filter(|f| f.status == Status::Sleeping && f.state == name)
            {
                let later = Env::new(f.wake_at, 7);
                step = ex.wake(&def, &later);
                continue;
            }
            break;
        }
        for d in calls {
            let outcome = outcome(shared, caller, r, &d).await;
            if let (Outcome::Done(v), 1) = (&outcome, d.frame) {
                seen.result = Some(v.clone());
            }
            let more = ex.deliver(&def, d.frame, d.seq, outcome, &env);
            step.events.extend(more.events);
            step.dispatches.extend(more.dispatches);
            step.end = step.end.take().or(more.end);
        }
    }
    let ran = Ran {
        def: &def,
        before: &before,
        after: &ex,
        env: &env,
    };
    Ok(answer(&ran, &name, &seen, r))
}

fn start(def: &Definition, r: &Value, caller: &Caller, input: String, now: i64) -> Exec {
    let context: Value = r
        .get("context")
        .and_then(Value::as_str)
        .and_then(|c| serde_json::from_str(c).ok())
        .unwrap_or(Value::Null);
    let text = |p: &str, d: &str| {
        context
            .pointer(p)
            .and_then(Value::as_str)
            .unwrap_or(d)
            .to_owned()
    };
    let machine = format!(
        "arn:aws:states:{}:{}:stateMachine:TestStateMachine",
        caller.region, caller.account_id
    );
    let s = Start {
        arn: text(
            "/Execution/Id",
            &format!("{machine}:TestExecution").replace(":stateMachine:", ":execution:"),
        ),
        name: text("/Execution/Name", "TestExecution"),
        machine_arn: text("/StateMachine/Id", &machine),
        machine_name: text("/StateMachine/Name", "TestStateMachine"),
        role_arn: r
            .get("roleArn")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        input,
        started_extra: Vec::new(),
    };
    let (mut ex, _) = Exec::start(def, s, now);
    if let Some(root) = ex.frames.first_mut() {
        if let Some(vars) = r.get("variables").and_then(Value::as_str) {
            root.vars = vars.to_owned();
        }
        if let Some(n) = r
            .pointer("/stateConfiguration/retrierRetryCount")
            .and_then(Value::as_u64)
        {
            let retriers = def
                .states
                .iter()
                .find(|s| s.name == def.start_at)
                .map_or(0, |s| s.retry.len());
            root.attempts = vec![u32::try_from(n).unwrap_or(0); retriers];
        }
    }
    ex
}

fn note(seen: &mut Seen, step: &Step) {
    for ev in &step.events {
        if (ev.kind.ends_with("Failed") || ev.kind.ends_with("TimedOut"))
            && let Some((_, d)) = &ev.details
        {
            let get = |k: &str| {
                d.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            seen.failure = Some((get("error"), get("cause")));
        }
    }
    if let Some(aws_asl::End::Failed(f) | aws_asl::End::TimedOut(f)) = &step.end {
        seen.failure = Some((f.error.clone(), f.cause.clone()));
    }
}

/// A call answered from `mock`, from the item reader data, or made for real as the role.
async fn outcome(shared: &Arc<Shared>, caller: &Caller, r: &Value, d: &Dispatch) -> Outcome {
    if d.resource == aws_asl::ITEM_READER
        && let Some(data) = r
            .pointer("/stateConfiguration/mapItemReaderData")
            .and_then(Value::as_str)
    {
        return Outcome::Done(serde_json::from_str(data).unwrap_or(Value::Null));
    }
    if let Some(mock) = r.get("mock") {
        if let Some(res) = mock.get("result").and_then(Value::as_str) {
            return Outcome::Done(
                serde_json::from_str(res).unwrap_or_else(|_| Value::String(res.to_owned())),
            );
        }
        if let Some(e) = mock.get("errorOutput") {
            let get = |k: &str| {
                e.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            return Outcome::Failed(aws_asl::Failure::new(get("error"), get("cause")));
        }
    }
    let Some(peers) = shared.peers.clone() else {
        return Outcome::Failed(crate::integ::not_local_failure(
            &d.resource,
            "no other service runs in this process",
        ));
    };
    let role = r.get("roleArn").and_then(Value::as_str).unwrap_or_default();
    let as_role = shared
        .sessions
        .caller(&peers, caller, role, shared.ctx.clock.now_ms())
        .await;
    let job = Job {
        execution: String::new(),
        frame: d.frame,
        seq: d.seq,
        resource: d.resource.clone(),
        input: d.input.clone(),
        token: d.token.clone(),
        credentials: d.credentials.clone(),
        role_arn: role.to_owned(),
        region: caller.region.clone(),
        account: caller.account_id.clone(),
        endpoint: caller.endpoint.clone(),
    };
    match crate::integ::call(&peers, &as_role, &job).await {
        Ok(v) => Outcome::Done(v),
        Err(f) => Outcome::Failed(f),
    }
}

/// A test run: the definition, the execution as it was before the state ran and as it is after, and the environment.
struct Ran<'a> {
    def: &'a Definition,
    before: &'a Exec,
    after: &'a Exec,
    env: &'a Env,
}

fn answer(run: &Ran<'_>, name: &str, seen: &Seen, r: &Value) -> Value {
    let mut out = Map::new();
    let root = run.after.frame(1).cloned().unwrap_or_default();
    let (status, next, output) = match root.status {
        Status::Done => ("SUCCEEDED", None, root.output.clone()),
        Status::RetryWait => ("RETRIABLE", None, None),
        Status::Failed | Status::Aborted => ("FAILED", None, None),
        _ if root.state != name => {
            let s = if seen.failure.is_some() {
                "CAUGHT_ERROR"
            } else {
                "SUCCEEDED"
            };
            (s, Some(root.state.clone()), Some(root.input.clone()))
        }
        _ => ("SUCCEEDED", None, root.task_input.clone()),
    };
    out.insert("status".into(), json!(status));
    if let Some(n) = next {
        out.insert("nextState".into(), json!(n));
    }
    if let Some(o) = output {
        out.insert("output".into(), json!(o));
    }
    if status != "SUCCEEDED"
        && let Some((e, c)) = &seen.failure
    {
        out.insert("error".into(), json!(e));
        out.insert("cause".into(), json!(c));
    }
    if matches!(
        r.get("inspectionLevel").and_then(Value::as_str),
        Some("DEBUG" | "TRACE")
    ) {
        out.insert("inspectionData".into(), inspection(run, seen, &root));
    }
    Value::Object(out)
}

fn inspection(run: &Ran<'_>, seen: &Seen, root: &aws_asl::exec::Frame) -> Value {
    let (def, before, env) = (run.def, run.before, run.env);
    let mut data = Map::new();
    data.insert("input".into(), json!(before.input));
    for (k, v) in before.inspect_input(def, 1, env) {
        data.insert(k.into(), json!(v.to_string()));
    }
    if let Some(res) = &seen.result {
        data.insert("result".into(), json!(res.to_string()));
        let mut at = before.clone();
        if let Some(f) = at.frames.first_mut() {
            f.state.clone_from(&def.start_at);
        }
        for (k, v) in at.inspect_result(def, 1, res, env) {
            data.insert(k.into(), json!(v.to_string()));
        }
    }
    data.insert("variables".into(), json!(root.vars));
    if root.status == Status::RetryWait {
        let wait = (root.wake_at - env.now_ms).max(0) / 1000;
        let index = root.attempts.iter().position(|a| *a > 0).unwrap_or(0);
        data.insert(
            "errorDetails".into(),
            json!({"retryIndex": index, "retryBackoffIntervalSeconds": wait}),
        );
    }
    Value::Object(data)
}
