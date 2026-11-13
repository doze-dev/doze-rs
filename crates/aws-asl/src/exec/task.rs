//! Task states: the call (with its token, timeout and heartbeat), its answer, and what a failure does: `Retry` (the first retrier
//! whose `ErrorEquals` matches decides; an exhausted one does not fall through), then `Catch`, then the branch fails.

use serde_json::{Value, json};

use super::io::{View, parse, text};
use super::run::{Move, jsonata_exit, number};
use super::{Cx, Dispatch, Exec, Outcome, Status, Step, sub};
use crate::def::{Definition, Fail, Kind, Lang, State};
use crate::env::Env;
use crate::errors::{Failure, RUNTIME, matches};

/// Whether a resource waits for a token: `.waitForTaskToken`, or an activity.
#[must_use]
pub fn parks(resource: &str) -> bool {
    resource.ends_with(".waitForTaskToken")
        || (resource.starts_with("arn:aws:states:") && resource.contains(":activity:"))
}

impl Exec {
    pub(crate) fn call_task(&mut self, id: u32, state: &State, env: &Env, step: &mut Step) -> Move {
        let Kind::Task(task) = &state.kind else {
            return Move::Parked;
        };
        let Some(f) = self.frame(id).cloned() else {
            return Move::Parked;
        };
        let token = parks(&task.resource).then(|| env.token());
        if token.is_some() && !env.tokens {
            // (memory)
            return Move::Fail(Failure::runtime(
                "Express state machines do not support the .waitForTaskToken pattern",
            ));
        }
        let view = View {
            ex: self,
            frame: &f,
            state,
            env,
        };
        let raw = parse(&f.input);
        let prepared = (|| -> Result<_, Failure> {
            let (input, eff) = if state.lang == Lang::Jsonata {
                let mut states = view.states(&raw, &[]);
                if let (Some(t), Some(ctx)) = (&token, states.pointer_mut("/context")) {
                    ctx["Task"] = json!({"Token": t});
                }
                let input = match &state.arguments {
                    Some(a) => view
                        .jsonata("Arguments", a, &states)?
                        .unwrap_or(Value::Null),
                    None => raw.clone(),
                };
                (input, raw.clone())
            } else {
                let eff = view.effective(&raw)?;
                let input = match &state.parameters {
                    Some(p) => view.template(p, &eff, token.as_deref())?,
                    None => eff.clone(),
                };
                (input, eff)
            };
            let timeout = number(&view, &eff, &task.timeout, "TimeoutSeconds")?;
            let heartbeat =
                number(&view, &eff, &task.heartbeat, "HeartbeatSeconds")?.map(|h| h as i64);
            let credentials = match &task.credentials {
                Some(c) if state.lang == Lang::Jsonata => {
                    view.jsonata("Credentials", c, &view.states(&raw, &[]))?
                }
                Some(c) => Some(view.template(c, &eff, None)?),
                None => None,
            };
            Ok((input, timeout, heartbeat, credentials))
        })();
        let (input, timeout, heartbeat, credentials) = match prepared {
            Ok(p) => p,
            Err(f) => return Move::Fail(f),
        };
        let input_text = text(&input);
        let Some(fm) = self.frame_mut(id) else {
            return Move::Parked;
        };
        fm.task_input = Some(input_text);
        fm.token = token;
        fm.heartbeat_secs = heartbeat.unwrap_or(0);
        self.dispatch(id, state, &task.resource, timeout, credentials, env, step);
        Move::Parked
    }

    /// Sends (or sends again, after a retry's wait) the frame's task call.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dispatch(
        &mut self,
        id: u32,
        state: &State,
        resource: &str,
        timeout: Option<f64>,
        credentials: Option<Value>,
        env: &Env,
        step: &mut Step,
    ) {
        let Some(f) = self.frame_mut(id) else { return };
        f.seq += 1;
        f.status = if f.token.is_some() {
            Status::Parked
        } else {
            Status::Calling
        };
        f.deadline = timeout.map_or(0, |t| env.now_ms + (t * 1000.0) as i64);
        f.heartbeat_at = if f.heartbeat_secs > 0 {
            env.now_ms + f.heartbeat_secs * 1000
        } else {
            0
        };
        let heartbeat = (f.heartbeat_secs > 0).then_some(f.heartbeat_secs);
        let d = Dispatch {
            frame: id,
            seq: f.seq,
            state: state.name.clone(),
            resource: resource.to_owned(),
            input: parse(f.task_input.as_deref().unwrap_or("null")),
            token: f.token.clone(),
            credentials,
            timeout_secs: timeout,
            heartbeat_secs: heartbeat,
        };
        let input_text = f.task_input.clone().unwrap_or_default();
        self.recorder(step, env)
            .scheduled(id, resource, &input_text, timeout, heartbeat);
        Self::push_dispatch(step, d);
    }

    /// The calls that are out (frames waiting for a call's answer, not for a token): what a service that restarted calls again.
    /// A task is called at least once, as on AWS.
    #[must_use]
    pub fn calls(&self, def: &Definition) -> Vec<Dispatch> {
        self.frames
            .iter()
            .filter(|f| f.status == Status::Calling)
            .filter_map(|f| {
                let resource = if f.proxy {
                    super::items::MAP_ITEM.to_owned()
                } else {
                    resource_of(def, f)?
                };
                Some(Dispatch {
                    frame: f.id,
                    seq: f.seq,
                    state: f.state.clone(),
                    resource,
                    input: parse(f.task_input.as_deref().unwrap_or("null")),
                    token: f.token.clone(),
                    credentials: None,
                    timeout_secs: None,
                    heartbeat_secs: (f.heartbeat_secs > 0).then_some(f.heartbeat_secs),
                })
            })
            .collect()
    }
}

/// The resource a calling frame called.
fn resource_of(def: &Definition, f: &super::Frame) -> Option<String> {
    let state = sub(def, &f.def)?.state(&f.state)?;
    Some(match &state.kind {
        Kind::Task(t) => t.resource.clone(),
        Kind::Map(_) if f.writing => super::writer::RESULT_WRITER.to_owned(),
        Kind::Map(_) => super::fan::ITEM_READER.to_owned(),
        _ => return None,
    })
}

impl Exec {
    /// The answer to a task call (or a token's): `seq` must be the frame's current call, or it is an old one and is ignored.
    pub fn deliver(
        &mut self,
        def: &Definition,
        frame: u32,
        seq: u32,
        outcome: Outcome,
        env: &Env,
    ) -> Step {
        let cx = Cx { def, env };
        let mut step = Step::default();
        let Some(f) = self.frame(frame).cloned() else {
            return step;
        };
        if f.seq != seq || !matches!(f.status, Status::Calling | Status::Parked) {
            return step;
        }
        if f.proxy {
            self.item_ended(cx, &f, outcome, &mut step);
            self.pump(cx, &mut step);
            return step;
        }
        let Some(state) = sub(def, &f.def).and_then(|d| d.state(&f.state)) else {
            return step;
        };
        if let Kind::Map(_) = &state.kind {
            self.map_answer(cx, &f, state, outcome, &mut step);
            self.pump(cx, &mut step);
            return step;
        }
        let Kind::Task(task) = &state.kind else {
            return step;
        };
        match outcome {
            Outcome::Submitted(answer) => {
                self.recorder(&mut step, env)
                    .submitted(frame, &task.resource, &text(&answer));
                if let Some(fm) = self.frame_mut(frame) {
                    fm.status = Status::Parked;
                }
                return step;
            }
            Outcome::Done(result) => {
                self.recorder(&mut step, env)
                    .succeeded(frame, &task.resource, &text(&result));
                let view = View {
                    ex: self,
                    frame: &f,
                    state,
                    env,
                };
                let mv = finish_result(&view, &parse(&f.input), result);
                self.apply(cx, frame, state, mv, &mut step);
            }
            Outcome::Failed(failure) => {
                self.recorder(&mut step, env).failed(
                    frame,
                    &task.resource,
                    &failure.error,
                    &failure.cause,
                );
                self.failed(cx, frame, state, failure, &mut step);
            }
        }
        self.pump(cx, &mut step);
        step
    }

    /// The answer to a distributed Map's reader (its items) or writer (its result).
    fn map_answer(
        &mut self,
        cx: Cx<'_>,
        f: &super::Frame,
        state: &State,
        outcome: Outcome,
        step: &mut Step,
    ) {
        let (error, writing) = if f.writing {
            (crate::errors::RESULT_WRITER_FAILED, true)
        } else {
            (crate::errors::ITEM_READER_FAILED, false)
        };
        if let Some(fm) = self.frame_mut(f.id) {
            fm.writing = false;
        }
        match outcome {
            Outcome::Done(result) if writing => {
                let view = View {
                    ex: self,
                    frame: f,
                    state,
                    env: cx.env,
                };
                let mv = finish_result(&view, &parse(&f.input), result);
                self.apply(cx, f.id, state, mv, step);
            }
            Outcome::Done(items) => self.items_read(cx, f.id, state, items, step),
            Outcome::Failed(failure) => {
                self.failed(cx, f.id, state, Failure::new(error, failure.cause), step);
            }
            Outcome::Submitted(_) => {}
        }
    }

    /// A state failed: retry, catch, or fail the branch.
    pub(crate) fn failed(
        &mut self,
        cx: Cx<'_>,
        id: u32,
        state: &State,
        failure: Failure,
        step: &mut Step,
    ) {
        let env = cx.env;
        if let Some(until) = self.retry(id, state, &failure, env) {
            if let Some(f) = self.frame_mut(id) {
                f.status = Status::RetryWait;
                f.wake_at = until;
            }
            return;
        }
        let Some(c) = state
            .catch
            .iter()
            .find(|c| matches(&failure.error, &c.errors))
        else {
            self.fail_frame(cx, id, failure, step);
            return;
        };
        let Some(f) = self.frame(id).cloned() else {
            return;
        };
        let view = View {
            ex: self,
            frame: &f,
            state,
            env,
        };
        let raw = parse(&f.input);
        let error_output = json!({"Error": failure.error, "Cause": failure.cause});
        let caught = (|| -> Result<Move, Failure> {
            if state.lang == Lang::Jsonata {
                let states = view.states(&raw, &[("errorOutput", error_output.clone())]);
                let output = match &c.output {
                    Some(o) => view.jsonata("Output", o, &states)?.unwrap_or(Value::Null),
                    None => error_output.clone(),
                };
                let assigned = view.assign(c.assign.as_ref(), &raw, &states)?;
                return Ok(Move::Exit {
                    output,
                    next: Some(c.next.clone()),
                    assigned,
                });
            }
            let output = view.merge(&c.result_path, &raw, error_output.clone())?;
            let assigned = view.assign(c.assign.as_ref(), &error_output, &Value::Null)?;
            Ok(Move::Exit {
                output,
                next: Some(c.next.clone()),
                assigned,
            })
        })();
        match caught {
            Ok(mv) => self.apply(cx, id, state, mv, step),
            Err(f) => self.fail_frame(cx, id, f, step),
        }
    }

    /// When the frame tries again, if a retrier takes the failure.
    fn retry(&mut self, id: u32, state: &State, failure: &Failure, env: &Env) -> Option<i64> {
        let (i, r) = state
            .retry
            .iter()
            .enumerate()
            .find(|(_, r)| matches(&failure.error, &r.errors))?;
        let f = self.frame_mut(id)?;
        if f.attempts.len() < state.retry.len() {
            f.attempts.resize(state.retry.len(), 0);
        }
        let used = *f.attempts.get(i)?;
        if used >= r.max_attempts {
            return None;
        }
        if let Some(a) = f.attempts.get_mut(i) {
            *a += 1;
        }
        let mut secs = r.interval * r.backoff.powi(i32::try_from(used).unwrap_or(i32::MAX));
        if let Some(cap) = r.max_delay {
            secs = secs.min(cap);
        }
        if r.jitter {
            secs *= env.unit();
        }
        Some(env.now_ms + (secs * env.wait_scale * 1000.0) as i64)
    }
}

/// A task's (or a fan-out's) result through `ResultSelector`, `ResultPath` and `OutputPath`, or JSONata's `Output`.
pub(crate) fn finish_result(v: &View<'_>, raw: &Value, result: Value) -> Move {
    let attempt = || -> Result<Move, Failure> {
        if v.state.lang == Lang::Jsonata {
            return jsonata_exit(v, raw, &[("result", result.clone())]);
        }
        let assigned = v.assign(v.state.assign.as_ref(), &result, &Value::Null)?;
        let selected = match &v.state.result_selector {
            Some(s) => v.template(s, &result, None)?,
            None => result,
        };
        let merged = v.merge(&v.state.result_path, raw, selected)?;
        let output = v.output(&merged)?;
        Ok(Move::Exit {
            output,
            next: v.state.next.clone(),
            assigned,
        })
    };
    attempt().unwrap_or_else(Move::Fail)
}

/// A Fail state's error and cause.
pub(crate) fn fail_state(v: &View<'_>, raw: &Value, spec: &Fail) -> Failure {
    let pick = |fixed: Option<&Value>,
                path: Option<&String>,
                field: &str|
     -> Result<Option<String>, Failure> {
        if let Some(p) = path {
            let got = super::run::select(v, raw, p, field)?;
            return got
                .as_str()
                .map(|s| Some(s.to_owned()))
                .ok_or_else(|| v.fail(RUNTIME, &format!("The value of {field} must be a string"))); // (memory)
        }
        match fixed {
            Some(e @ Value::String(s)) if crate::jsonata::is_expr(s) => Ok(v
                .jsonata(field, e, &v.states(raw, &[]))?
                .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_owned))),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            _ => Ok(None),
        }
    };
    let error = match pick(spec.error.as_ref(), spec.error_path.as_ref(), "ErrorPath") {
        Ok(e) => e.unwrap_or_default(),
        Err(f) => return f,
    };
    let cause = match pick(spec.cause.as_ref(), spec.cause_path.as_ref(), "CausePath") {
        Ok(c) => c.unwrap_or_default(),
        Err(f) => return f,
    };
    Failure::new(error, cause)
}
