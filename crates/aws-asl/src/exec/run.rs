//! Running the frames that can run, one state at a time: Pass, Choice, Wait, Succeed and Fail here; Task in `task.rs`, Parallel
//! and Map in `fan.rs`.

use serde_json::{Map, Value, json};

use super::events::Recorder;
use super::io::{View, parse, set_vars, text};
use super::{Cx, Dispatch, End, Exec, Frame, Status, Step, sub};
use crate::def::{Definition, Kind, Lang, Num, State, TimeSource, Wait};
use crate::env::Env;
use crate::errors::{Failure, NO_CHOICE_MATCHED, RUNTIME};

/// How many states one call may run before the execution is failed (a loop of Pass states would otherwise never yield).
const MOST_TRANSITIONS: u32 = 25_000;

/// A state's output, where it goes, and the variables it set.
pub(crate) type Exited = (Value, Option<String>, Option<Map<String, Value>>);

/// What running a state led to.
pub(crate) enum Move {
    /// The state is done with `output`, and goes to `next` (or ends its branch).
    Exit {
        output: Value,
        next: Option<String>,
        assigned: Option<Map<String, Value>>,
    },
    /// The state failed (a Task, Parallel or Map state may still retry or catch).
    Fail(Failure),
    /// The frame waits until then.
    Sleep(i64),
    /// The frame was given to a task, a fan-out or a timer: nothing more to do now.
    Parked,
}

impl Exec {
    /// Runs every frame that can run, until each waits for something or the execution ends.
    pub fn run(&mut self, def: &Definition, env: &Env) -> Step {
        let mut step = Step::default();
        self.pump(Cx { def, env }, &mut step);
        step
    }

    pub(crate) fn pump(&mut self, cx: Cx<'_>, step: &mut Step) {
        let env = cx.env;
        let mut budget = MOST_TRANSITIONS;
        while step.end.is_none() {
            let Some(id) = self
                .frames
                .iter()
                .find(|f| f.status == Status::Runnable)
                .map(|f| f.id)
            else {
                return;
            };
            if budget == 0 {
                self.end_failed(
                    Failure::runtime("The execution made too many state transitions in a row"),
                    env,
                    step,
                );
                return;
            }
            budget -= 1;
            self.advance(cx, id, step);
        }
    }

    pub(crate) fn recorder<'a>(&'a mut self, step: &'a mut Step, env: &Env) -> Recorder<'a> {
        Recorder {
            ex: self,
            out: &mut step.events,
            now: env.now_ms,
            region: env.region.clone(),
        }
    }

    fn advance(&mut self, cx: Cx<'_>, id: u32, step: &mut Step) {
        let (def, env) = (cx.def, cx.env);
        let Some((sub_def, name)) = self.frame(id).and_then(|f| {
            let d = sub(def, &f.def)?;
            let name = if f.state.is_empty() {
                d.start_at.clone()
            } else {
                f.state.clone()
            };
            Some((d, name))
        }) else {
            self.fail_frame(
                cx,
                id,
                Failure::runtime("The frame names a state that does not exist"),
                step,
            );
            return;
        };
        let Some(state) = sub_def.state(&name) else {
            self.fail_frame(
                cx,
                id,
                Failure::runtime(format!("The state {name} does not exist")),
                step,
            );
            return;
        };
        if let Some(f) = self.frame_mut(id) {
            f.state = name;
            f.entered_ms = env.now_ms;
        }
        let snapshot = self.frame(id).cloned().unwrap_or_default();
        let entered = self
            .recorder(step, env)
            .entered(&snapshot, state.type_name());
        if let Some(f) = self.frame_mut(id) {
            f.entered_event = entered;
        }
        let mv = self.enter(cx, id, state, step);
        self.apply(cx, id, state, mv, step);
    }

    fn enter(&mut self, cx: Cx<'_>, id: u32, state: &State, step: &mut Step) -> Move {
        let env = cx.env;
        let Some(f) = self.frame(id).cloned() else {
            return Move::Parked;
        };
        let view = View {
            ex: self,
            frame: &f,
            state,
            env,
        };
        let raw = parse(&f.input);
        match &state.kind {
            Kind::Pass { result } => pass(&view, &raw, result.as_ref()),
            Kind::Choice { rules, default } => {
                super::choose::choose(&view, &raw, rules, default.as_deref())
            }
            Kind::Wait(w) => wait(&view, &raw, w),
            Kind::Succeed => finish_plain(&view, &raw, None),
            Kind::Fail(spec) => Move::Fail(super::task::fail_state(&view, &raw, spec)),
            Kind::Task(_) => self.call_task(id, state, env, step),
            Kind::Parallel { .. } | Kind::Map(_) => self.fan_out(cx, id, state, step),
        }
    }

    /// Carries out what a state led to.
    pub(crate) fn apply(&mut self, cx: Cx<'_>, id: u32, state: &State, mv: Move, step: &mut Step) {
        match mv {
            Move::Exit {
                output,
                next,
                assigned,
            } => self.exit(cx, id, state, (output, next, assigned), step),
            Move::Fail(failure) => self.failed(cx, id, state, failure, step),
            Move::Sleep(until) => {
                if let Some(f) = self.frame_mut(id) {
                    f.status = Status::Sleeping;
                    f.wake_at = until;
                }
            }
            Move::Parked => {}
        }
    }

    /// The state is done: its `...StateExited`, then the next state or the end of the branch.
    pub(crate) fn exit(
        &mut self,
        cx: Cx<'_>,
        id: u32,
        state: &State,
        done: Exited,
        step: &mut Step,
    ) {
        let (output, next, assigned) = done;
        let assigned = assigned.as_ref();
        let out = text(&output);
        self.recorder(step, cx.env)
            .exited(id, &state.name, state.type_name(), &out, assigned);
        let Some(f) = self.frame_mut(id) else { return };
        if let Some(a) = assigned {
            set_vars(f, a);
        }
        reset(f);
        match next {
            Some(n) => {
                f.state = n;
                f.input = out;
                f.status = Status::Runnable;
            }
            None => {
                f.status = Status::Done;
                f.output = Some(out);
                self.frame_ended(cx, id, step);
            }
        }
    }

    /// The frame failed with no retry or catch left.
    pub(crate) fn fail_frame(&mut self, cx: Cx<'_>, id: u32, failure: Failure, step: &mut Step) {
        if let Some(f) = self.frame_mut(id) {
            f.status = Status::Failed;
            f.failure = Some(failure);
        }
        self.frame_ended(cx, id, step);
    }

    /// A frame finished: the root ends the execution, a branch is joined.
    pub(crate) fn frame_ended(&mut self, cx: Cx<'_>, id: u32, step: &mut Step) {
        let env = cx.env;
        let Some(f) = self.frame(id).cloned() else {
            return;
        };
        if f.parent != 0 {
            self.joined(cx, f.parent, id, step);
            return;
        }
        match (f.status, f.failure) {
            (Status::Failed, Some(failure)) => self.end_failed(failure, env, step),
            _ => {
                let output = f.output.unwrap_or_else(|| "null".to_owned());
                self.recorder(step, env).add(
                    1,
                    "ExecutionSucceeded",
                    Some((
                        "executionSucceededEventDetails",
                        json!({"output": output, "outputDetails": super::events::whole()}),
                    )),
                );
                step.end = Some(End::Succeeded(output));
            }
        }
    }

    pub(crate) fn end_failed(&mut self, failure: Failure, env: &Env, step: &mut Step) {
        for f in &mut self.frames {
            if !f.status.finished() {
                f.status = Status::Aborted;
            }
        }
        self.recorder(step, env).add(
            1,
            "ExecutionFailed",
            Some((
                "executionFailedEventDetails",
                json!({"error": failure.error, "cause": failure.cause}),
            )),
        );
        step.end = Some(End::Failed(failure));
    }

    pub(crate) fn push_dispatch(step: &mut Step, d: Dispatch) {
        step.dispatches.push(d);
    }
}

/// Clears what belonged to the state just left.
pub(crate) fn reset(f: &mut Frame) {
    f.attempts.clear();
    f.task_input = None;
    f.token = None;
    f.wake_at = 0;
    f.deadline = 0;
    f.heartbeat_at = 0;
    f.heartbeat_secs = 0;
    f.limit = 0;
    f.tolerated = None;
}

fn pass(v: &View<'_>, raw: &Value, result: Option<&Value>) -> Move {
    let attempt = || -> Result<Move, Failure> {
        if v.state.lang == Lang::Jsonata {
            return jsonata_exit(v, raw, &[]);
        }
        let mut eff = v.effective(raw)?;
        if let Some(p) = &v.state.parameters {
            eff = v.template(p, &eff, None)?;
        }
        let made = result.cloned().unwrap_or_else(|| eff.clone());
        let merged = v.merge(&v.state.result_path, raw, made)?;
        let output = v.output(&merged)?;
        let assigned = v.assign(v.state.assign.as_ref(), &eff, &Value::Null)?;
        Ok(Move::Exit {
            output,
            next: v.state.next.clone(),
            assigned,
        })
    };
    attempt().unwrap_or_else(Move::Fail)
}

/// A JSONata state's `Output` (its input when there is none) and `Assign`, with `extra` in `$states`.
pub(crate) fn jsonata_exit(
    v: &View<'_>,
    raw: &Value,
    extra: &[(&str, Value)],
) -> Result<Move, Failure> {
    let states = v.states(raw, extra);
    let fallback = extra
        .iter()
        .find(|(k, _)| *k == "result")
        .map_or_else(|| raw.clone(), |(_, r)| r.clone());
    let output = match &v.state.output {
        Some(o) => v.jsonata("Output", o, &states)?.unwrap_or(Value::Null),
        None => fallback,
    };
    let assigned = v.assign(v.state.assign.as_ref(), raw, &states)?;
    Ok(Move::Exit {
        output,
        next: v.state.next.clone(),
        assigned,
    })
}

/// Succeed, or a Wait that is over: `InputPath`, then `OutputPath` (JSONPath), or `Output` (JSONata).
pub(crate) fn finish_plain(v: &View<'_>, raw: &Value, next: Option<String>) -> Move {
    let attempt = || -> Result<Move, Failure> {
        if v.state.lang == Lang::Jsonata {
            let mut mv = jsonata_exit(v, raw, &[])?;
            if let Move::Exit { next: n, .. } = &mut mv {
                n.clone_from(&next);
            }
            return Ok(mv);
        }
        let eff = v.effective(raw)?;
        let output = v.output(&eff)?;
        let assigned = v.assign(v.state.assign.as_ref(), &eff, &Value::Null)?;
        Ok(Move::Exit {
            output,
            next,
            assigned,
        })
    };
    attempt().unwrap_or_else(Move::Fail)
}

fn wait(v: &View<'_>, raw: &Value, w: &Wait) -> Move {
    match until(v, raw, w) {
        Ok(at) if at > v.env.now_ms => Move::Sleep(at),
        Ok(_) => finish_plain(v, raw, v.state.next.clone()),
        Err(f) => Move::Fail(f),
    }
}

/// When a Wait state ends.
fn until(v: &View<'_>, raw: &Value, w: &Wait) -> Result<i64, Failure> {
    let now = v.env.now_ms;
    let eff = if v.state.lang == Lang::Jsonata {
        raw.clone()
    } else {
        v.effective(raw)?
    };
    match w {
        Wait::Seconds(n) => {
            let secs = number(v, &eff, n, "Seconds")?.unwrap_or(0.0);
            if secs < 0.0 {
                return Err(v.fail(RUNTIME, "The value of Seconds must be a positive integer")); // (memory)
            }
            Ok(now + (secs * v.env.wait_scale * 1000.0) as i64)
        }
        Wait::Timestamp(src) => {
            let text = match src {
                TimeSource::Fixed(t) => Value::String(t.clone()),
                TimeSource::Path(p) => select(v, &eff, p, "TimestampPath")?,
                TimeSource::Expr(e) => v
                    .jsonata("Timestamp", e, &v.states(raw, &[]))?
                    .unwrap_or(Value::Null),
            };
            let scale = v.env.wait_scale;
            let scaled = |ts: i64| {
                if ts > now {
                    now + ((ts - now) as f64 * scale) as i64
                } else {
                    ts
                }
            };
            text.as_str()
                .and_then(crate::time::parse)
                .map(scaled)
                .ok_or_else(|| {
                    v.fail(
                        RUNTIME,
                        &format!(
                            "The value '{text}' of Timestamp is not a valid ISO-8601 timestamp"
                        ),
                    )
                }) // (memory)
        }
    }
}

/// A single value at a path of the effective input.
pub(crate) fn select(v: &View<'_>, eff: &Value, p: &str, field: &str) -> Result<Value, Failure> {
    let (ctx, vars) = (v.context(None), v.vars());
    crate::path::Path::parse(p)
        .ok()
        .and_then(|path| path.select(crate::path::Scope { data: eff, context: &ctx, vars: &vars }))
        .ok_or_else(|| {
            v.fail(
                RUNTIME,
                &format!("The JSONPath '{p}' specified for the field '{field}' could not be found in the input '{eff}'"),
            )
        })
}

/// A number field: fixed, at a path, or a JSONata expression.
pub(crate) fn number(
    v: &View<'_>,
    eff: &Value,
    n: &Num,
    field: &str,
) -> Result<Option<f64>, Failure> {
    let got = match n {
        Num::None => return Ok(None),
        Num::Fixed(x) => return Ok(Some(*x)),
        Num::Path(p) => select(v, eff, p, &format!("{field}Path"))?,
        Num::Expr(e) => v
            .jsonata(field, e, &v.states(eff, &[]))?
            .unwrap_or(Value::Null),
    };
    got.as_f64().map(Some).ok_or_else(|| {
        v.fail(
            RUNTIME,
            &format!("The value of {field} must be a number, not {got}"),
        )
    }) // (memory)
}

/// Why a Choice state found no rule (memory).
pub(crate) fn no_match() -> Failure {
    Failure::new(NO_CHOICE_MATCHED, "No Matches!")
}
