//! Parallel and Map states: a frame per branch or item, run beside each other, joined into an array in order. A failed branch fails
//! the state (its siblings are stopped) unless a distributed Map tolerates it. A Map runs at most `MaxConcurrency` items at once.
//! A distributed Map's items may come from an `ItemReader`, which the service reads (it is I/O) and hands back to [`Exec::deliver`].

use serde_json::{Value, json};

use super::io::{View, parse, text};
use super::plan::{Plan, map_run_arn, plan, read_items};
use super::run::Move;
use super::task::finish_result;
use super::{Cx, Dispatch, Exec, Frame, Hop, Status, Step, sub};
use crate::def::{Kind, State};
use crate::env::Env;
use crate::errors::{EXCEED_TOLERATED_FAILURE_THRESHOLD, Failure};

/// The resource of the call that reads a distributed Map's items.
pub const ITEM_READER: &str = "doze:itemReader";

impl Exec {
    pub(crate) fn fan_out(&mut self, cx: Cx<'_>, id: u32, state: &State, step: &mut Step) -> Move {
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
        if let Kind::Map(m) = &state.kind
            && let Some(reader) = &m.item_reader
        {
            return match read_items(&view, &raw, reader) {
                Ok(input) => {
                    self.read(id, state, input, env, step);
                    Move::Parked
                }
                Err(e) => Move::Fail(e),
            };
        }
        match plan(&view, &raw, None) {
            Ok(p) => {
                self.spawn(cx, id, state, p, step);
                Move::Parked
            }
            Err(e) => Move::Fail(e),
        }
    }

    /// Asks the service for a distributed Map's items.
    fn read(&mut self, id: u32, state: &State, input: Value, _env: &Env, step: &mut Step) {
        let Some(f) = self.frame_mut(id) else { return };
        f.seq += 1;
        f.status = Status::Calling;
        f.task_input = Some(text(&input));
        Self::push_dispatch(
            step,
            Dispatch {
                frame: id,
                seq: f.seq,
                state: state.name.clone(),
                resource: ITEM_READER.to_owned(),
                input,
                token: None,
                credentials: None,
                timeout_secs: None,
                heartbeat_secs: None,
            },
        );
    }

    /// The items a reader read: the Map runs over them.
    pub(crate) fn items_read(
        &mut self,
        cx: Cx<'_>,
        id: u32,
        state: &State,
        items: Value,
        step: &mut Step,
    ) {
        let env = cx.env;
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
        match plan(&view, &raw, Some(items)) {
            Ok(p) => self.spawn(cx, id, state, p, step),
            Err(e) => self.failed(cx, id, state, e, step),
        }
    }

    fn spawn(&mut self, cx: Cx<'_>, id: u32, state: &State, plan: Plan, step: &mut Step) {
        let env = cx.env;
        let distributed = matches!(&state.kind, Kind::Map(m) if m.distributed);
        // The limits first: a Map Run is described from the frame when its event is kept.
        if let Some(parent) = self.frame_mut(id) {
            parent.limit = plan.limit;
            parent.tolerated = plan.tolerated;
        }
        let started = match &state.kind {
            Kind::Parallel { .. } => self
                .recorder(step, env)
                .add(id, "ParallelStateStarted", None),
            Kind::Map(m) if m.distributed => {
                let run = map_run_arn(self, state, m, env);
                let ev = self.recorder(step, env).add(
                    id,
                    "MapRunStarted",
                    Some(("mapRunStartedEventDetails", json!({"mapRunArn": run}))),
                );
                if let Some(f) = self.frame_mut(id) {
                    f.map_run = Some(run);
                }
                ev
            }
            _ => self.recorder(step, env).add(
                id,
                "MapStateStarted",
                Some((
                    "mapStateStartedEventDetails",
                    json!({"length": plan.inputs.len()}),
                )),
            ),
        };
        let Some(parent) = self.frame_mut(id) else {
            return;
        };
        parent.fanouts += 1;
        parent.status = Status::Join;
        parent.started_event = started;
        parent.limit = plan.limit;
        parent.tolerated = plan.tolerated;
        let parent = parent.clone();
        let is_map = matches!(state.kind, Kind::Map(_));
        for (i, (input, item)) in plan.inputs.into_iter().enumerate() {
            let mut hops = parent.def.clone();
            hops.push(Hop {
                state: state.name.clone(),
                branch: if is_map { None } else { u32::try_from(i).ok() },
            });
            let child = Frame {
                id: self.next_frame,
                parent: id,
                index: u32::try_from(i).unwrap_or(u32::MAX),
                generation: parent.fanouts,
                def: hops,
                status: if is_map {
                    Status::Pending
                } else {
                    Status::Runnable
                },
                input: text(&input),
                item: item.map(|v| text(&v)),
                prev_event: started,
                vars: parent.vars.clone(),
                proxy: distributed,
                ..Frame::default()
            };
            self.next_frame += 1;
            self.frames.push(child);
        }
        self.promote(id, state, env, step);
        self.joined(cx, id, 0, step);
    }

    /// Starts held-back Map items while the limit allows.
    fn promote(&mut self, id: u32, state: &State, env: &Env, step: &mut Step) {
        let Some(parent) = self.frame(id).cloned() else {
            return;
        };
        let inline = matches!(&state.kind, Kind::Map(m) if !m.distributed);
        let running = self
            .frames
            .iter()
            .filter(|f| {
                f.parent == id
                    && f.generation == parent.fanouts
                    && !f.status.finished()
                    && f.status != Status::Pending
            })
            .count();
        let room = if parent.limit == 0 {
            usize::MAX
        } else {
            (parent.limit as usize).saturating_sub(running)
        };
        let pending: Vec<u32> = self
            .frames
            .iter()
            .filter(|f| {
                f.parent == id && f.generation == parent.fanouts && f.status == Status::Pending
            })
            .take(room)
            .map(|f| f.id)
            .collect();
        for child in pending {
            if self.frame(child).is_some_and(|c| c.proxy) {
                self.start_item(child, state, step);
                continue;
            }
            if let Some(c) = self.frame_mut(child) {
                c.status = Status::Runnable;
                let index = c.index;
                if inline {
                    self.recorder(step, env).add(
                        child,
                        "MapIterationStarted",
                        Some((
                            "mapIterationStartedEventDetails",
                            json!({"name": state.name, "index": index}),
                        )),
                    );
                }
            }
        }
    }

    /// A branch ended (or, with `child` 0, the fan-out just began): fail, wait, or join.
    pub(crate) fn joined(&mut self, cx: Cx<'_>, id: u32, child: u32, step: &mut Step) {
        let (def, env) = (cx.def, cx.env);
        let Some(parent) = self.frame(id).cloned() else {
            return;
        };
        if parent.status != Status::Join {
            return;
        }
        let Some(state) = sub(def, &parent.def).and_then(|d| d.state(&parent.state)) else {
            return;
        };
        if child != 0 {
            self.iteration_ended(id, child, state, env, step);
        }
        let kids: Vec<Frame> = self
            .frames
            .iter()
            .filter(|f| f.parent == id && f.generation == parent.fanouts)
            .cloned()
            .collect();
        if let Some(failure) = too_many_failed(&kids, parent.tolerated, state) {
            self.fan_failed(cx, id, state, failure, step);
            return;
        }
        if kids.iter().all(|k| k.status.finished()) {
            self.fan_done(cx, id, state, &kids, step);
        } else {
            self.promote(id, state, env, step);
        }
    }

    fn iteration_ended(&mut self, id: u32, child: u32, state: &State, env: &Env, step: &mut Step) {
        let Kind::Map(m) = &state.kind else { return };
        if m.distributed {
            return;
        }
        let Some(c) = self.frame(child).cloned() else {
            return;
        };
        let kind = if c.status == Status::Done {
            "MapIterationSucceeded"
        } else {
            "MapIterationFailed"
        };
        let key = if c.status == Status::Done {
            "mapIterationSucceededEventDetails"
        } else {
            "mapIterationFailedEventDetails"
        };
        self.recorder(step, env).add(
            child,
            kind,
            Some((key, json!({"name": state.name, "index": c.index}))),
        );
        let _ = id;
    }

    fn fan_failed(
        &mut self,
        cx: Cx<'_>,
        id: u32,
        state: &State,
        failure: Failure,
        step: &mut Step,
    ) {
        let env = cx.env;
        let generation = self.frame(id).map_or(0, |f| f.fanouts);
        let inline = matches!(&state.kind, Kind::Map(m) if !m.distributed);
        let stopped: Vec<(u32, u32)> = self
            .frames
            .iter()
            .filter(|f| f.parent == id && f.generation == generation && !f.status.finished())
            .map(|f| (f.id, f.index))
            .collect();
        for (cid, index) in stopped {
            self.abort_frame(cid);
            if inline {
                self.recorder(step, env).add(
                    cid,
                    "MapIterationAborted",
                    Some((
                        "mapIterationAbortedEventDetails",
                        json!({"name": state.name, "index": index}),
                    )),
                );
            }
        }
        let kind = match &state.kind {
            Kind::Parallel { .. } => "ParallelStateFailed",
            Kind::Map(m) if m.distributed => "MapRunFailed",
            _ => "MapStateFailed",
        };
        let details = (kind == "MapRunFailed").then(|| {
            (
                "mapRunFailedEventDetails",
                json!({"error": failure.error, "cause": failure.cause}),
            )
        });
        self.recorder(step, env).add(id, kind, details);
        if let Some(f) = self.frame_mut(id) {
            f.status = Status::Calling;
        }
        self.failed(cx, id, state, failure, step);
    }

    fn fan_done(&mut self, cx: Cx<'_>, id: u32, state: &State, kids: &[Frame], step: &mut Step) {
        let env = cx.env;
        let mut sorted: Vec<&Frame> = kids.iter().collect();
        sorted.sort_by_key(|k| k.index);
        let result = Value::Array(
            sorted
                .iter()
                .map(|k| match (&k.output, &k.failure) {
                    (Some(o), _) => parse(o),
                    (None, Some(f)) => json!({"Error": f.error, "Cause": f.cause}),
                    _ => Value::Null,
                })
                .collect(),
        );
        let kind = match &state.kind {
            Kind::Parallel { .. } => "ParallelStateSucceeded",
            Kind::Map(m) if m.distributed => "MapRunSucceeded",
            _ => "MapStateSucceeded",
        };
        self.recorder(step, env).add(id, kind, None);
        if self.write_results(cx, id, state, &sorted, step) {
            return;
        }
        let Some(f) = self.frame(id).cloned() else {
            return;
        };
        let view = View {
            ex: self,
            frame: &f,
            state,
            env,
        };
        let mv = finish_result(&view, &parse(&f.input), result);
        if let Some(fm) = self.frame_mut(id) {
            fm.status = Status::Calling;
        }
        self.apply(cx, id, state, mv, step);
    }

    /// Stops a frame and every frame under it.
    pub(crate) fn abort_frame(&mut self, id: u32) {
        let below: Vec<u32> = self
            .frames
            .iter()
            .filter(|f| f.parent == id)
            .map(|f| f.id)
            .collect();
        for b in below {
            self.abort_frame(b);
        }
        if let Some(f) = self.frame_mut(id)
            && !f.status.finished()
        {
            f.status = Status::Aborted;
        }
    }
}

fn too_many_failed(
    kids: &[Frame],
    tolerated: Option<(f64, f64)>,
    state: &State,
) -> Option<Failure> {
    let failed: Vec<&Frame> = kids.iter().filter(|k| k.status == Status::Failed).collect();
    let first = failed.first()?;
    let Some((count, pct)) = tolerated else {
        return first
            .failure
            .clone()
            .or_else(|| Some(Failure::runtime("A branch failed")));
    };
    #[allow(clippy::cast_precision_loss)]
    let share = failed.len() as f64 * 100.0 / kids.len().max(1) as f64;
    if failed.len() as f64 > count && share > pct {
        // (memory)
        return Some(Failure::new(
            EXCEED_TOLERATED_FAILURE_THRESHOLD,
            format!(
                "The specified tolerated failure threshold was exceeded in the Map state '{}'",
                state.name
            ),
        ));
    }
    None
}
