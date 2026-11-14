//! Time: a Wait that is over, a retry's wait, a task's `TimeoutSeconds` and `HeartbeatSeconds`, the machine's own `TimeoutSeconds`;
//! and the execution stopped from outside. The service sleeps to [`Exec::next_due`] and calls [`Exec::wake`]: nothing ticks.

use serde_json::json;

use super::io::parse;
use super::run::{Move, finish_plain};
use super::{Cx, End, Exec, Status, Step, sub};
use crate::def::{Definition, Kind};
use crate::env::Env;
use crate::errors::{Failure, HEARTBEAT_TIMEOUT, TIMEOUT};

impl Exec {
    /// The earliest time something is due, if anything is.
    #[must_use]
    pub fn next_due(&self) -> Option<i64> {
        if self.finished() {
            return None;
        }
        let frames = self.frames.iter().flat_map(|f| {
            let wake =
                matches!(f.status, Status::Sleeping | Status::RetryWait).then_some(f.wake_at);
            let waiting = matches!(f.status, Status::Calling | Status::Parked);
            let deadline = (waiting && f.deadline > 0).then_some(f.deadline);
            let heartbeat = (waiting && f.heartbeat_at > 0).then_some(f.heartbeat_at);
            [wake, deadline, heartbeat]
        });
        frames
            .chain([(self.deadline > 0).then_some(self.deadline)])
            .flatten()
            .min()
    }

    /// Does what is due at `env.now_ms`.
    pub fn wake(&mut self, def: &Definition, env: &Env) -> Step {
        let cx = Cx { def, env };
        let mut step = Step::default();
        let now = env.now_ms;
        if self.deadline > 0 && now >= self.deadline && !self.finished() {
            self.timed_out(env, &mut step);
            return step;
        }
        let due: Vec<(u32, Status)> = self
            .frames
            .iter()
            .filter(|f| match f.status {
                Status::Sleeping | Status::RetryWait => f.wake_at <= now,
                Status::Calling | Status::Parked => {
                    (f.deadline > 0 && f.deadline <= now)
                        || (f.heartbeat_at > 0 && f.heartbeat_at <= now)
                }
                _ => false,
            })
            .map(|f| (f.id, f.status))
            .collect();
        for (id, status) in due {
            if step.end.is_some() {
                break;
            }
            self.wake_frame(cx, id, status, &mut step);
        }
        self.pump(cx, &mut step);
        step
    }

    fn wake_frame(&mut self, cx: Cx<'_>, id: u32, status: Status, step: &mut Step) {
        let (def, env) = (cx.def, cx.env);
        let Some(f) = self.frame(id).cloned() else {
            return;
        };
        let Some(state) = sub(def, &f.def).and_then(|d| d.state(&f.state)) else {
            return;
        };
        match status {
            Status::Sleeping => {
                let view = super::io::View {
                    ex: self,
                    frame: &f,
                    state,
                    env,
                };
                let mv = finish_plain(&view, &parse(&f.input), state.next.clone());
                if let Some(fm) = self.frame_mut(id) {
                    fm.status = Status::Calling;
                }
                self.apply(cx, id, state, mv, step);
            }
            Status::RetryWait => match &state.kind {
                Kind::Task(t) => {
                    let timeout = super::run::number(
                        &super::io::View {
                            ex: self,
                            frame: &f,
                            state,
                            env,
                        },
                        &parse(&f.input),
                        &t.timeout,
                        "TimeoutSeconds",
                    )
                    .ok()
                    .flatten();
                    self.dispatch(id, state, &t.resource, timeout, None, env, step);
                }
                _ => {
                    let mv = self.fan_out(cx, id, state, step);
                    if !matches!(mv, Move::Parked) {
                        self.apply(cx, id, state, mv, step);
                    }
                }
            },
            _ => {
                let error = if f.deadline > 0 && f.deadline <= env.now_ms {
                    TIMEOUT
                } else {
                    HEARTBEAT_TIMEOUT
                };
                // (memory) AWS leaves the cause of a task timeout empty.
                let step_out = self.deliver(
                    def,
                    id,
                    f.seq,
                    super::Outcome::Failed(Failure::new(error, "")),
                    env,
                );
                step.events.extend(step_out.events);
                step.dispatches.extend(step_out.dispatches);
                step.end = step.end.take().or(step_out.end);
            }
        }
    }

    fn timed_out(&mut self, env: &Env, step: &mut Step) {
        for f in &mut self.frames {
            if !f.status.finished() {
                f.status = Status::Aborted;
            }
        }
        let failure = Failure::new(TIMEOUT, "");
        self.recorder(step, env).add(
            1,
            "ExecutionTimedOut",
            Some(("executionTimedOutEventDetails", json!({"error": TIMEOUT}))),
        );
        step.end = Some(End::TimedOut(failure));
    }

    /// Stops the execution (`StopExecution`): every frame is aborted and `ExecutionAborted` recorded. The tokens it was waiting on
    /// are returned so that the service can forget them.
    pub fn abort(
        &mut self,
        error: Option<&str>,
        cause: Option<&str>,
        env: &Env,
    ) -> (Step, Vec<String>) {
        let mut step = Step::default();
        let mut tokens = Vec::new();
        for f in &mut self.frames {
            if !f.status.finished() {
                tokens.extend(f.token.take());
                f.status = Status::Aborted;
            }
        }
        let mut d = serde_json::Map::new();
        if let Some(e) = error {
            d.insert("error".into(), json!(e));
        }
        if let Some(c) = cause {
            d.insert("cause".into(), json!(c));
        }
        self.recorder(&mut step, env).add(
            1,
            "ExecutionAborted",
            Some(("executionAbortedEventDetails", serde_json::Value::Object(d))),
        );
        (step, tokens)
    }

    /// `SendTaskHeartbeat`: the frame's heartbeat clock starts again.
    pub fn heartbeat(&mut self, frame: u32, now_ms: i64) {
        if let Some(f) = self.frame_mut(frame)
            && f.heartbeat_secs > 0
        {
            f.heartbeat_at = now_ms + f.heartbeat_secs * 1000;
        }
    }

    /// A worker took an activity task: `ActivityStarted`.
    pub fn activity_started(&mut self, frame: u32, worker: Option<&str>, env: &Env) -> Step {
        let mut step = Step::default();
        let details = worker.map(|w| ("activityStartedEventDetails", json!({"workerName": w})));
        self.recorder(&mut step, env)
            .add(frame, "ActivityStarted", details);
        step
    }
}
