//! A distributed Map's items, run as executions of their own (as AWS runs them, so they are listed with the Map Run's ARN). Each item
//! is a frame that only stands for its execution: when the concurrency allows, the frame is "called" with the item processor's
//! definition and input, the service starts the execution, and the execution's end is the frame's answer. The join, the tolerance,
//! the counts and the result writer are the Map's as for any branch.

use serde_json::{Value, json};

use super::io::text;
use super::{Cx, Dispatch, Exec, Frame, Outcome, Status, Step};
use crate::def::{Kind, State};
use crate::env::Env;

/// The resource of the call that starts a distributed Map's item as an execution.
pub const MAP_ITEM: &str = "doze:mapItem";

impl Exec {
    /// Starts the execution of a held-back item.
    pub(crate) fn start_item(&mut self, child: u32, state: &State, step: &mut Step) {
        let Kind::Map(m) = &state.kind else { return };
        let map_run = self
            .frame(child)
            .and_then(|c| self.frame(c.parent))
            .and_then(|p| p.map_run.clone());
        // The item processor as a machine of its own: its `ProcessorConfig` is the Map's, and its language is the Map state's.
        let mut definition = m.processor_json.clone().unwrap_or(Value::Null);
        if let Some(d) = definition.as_object_mut() {
            d.remove("ProcessorConfig");
            if state.lang == crate::def::Lang::Jsonata {
                d.insert("QueryLanguage".into(), json!("JSONata"));
            }
        }
        let Some(c) = self.frame_mut(child) else {
            return;
        };
        let input = json!({
            "Definition": definition.to_string(),
            "Input": c.input,
            "Index": c.index,
            "MapRunArn": map_run,
            "ExecutionType": m.execution_type,
        });
        c.seq += 1;
        c.status = Status::Calling;
        c.task_input = Some(text(&input));
        let d = Dispatch {
            frame: child,
            seq: c.seq,
            state: state.name.clone(),
            resource: MAP_ITEM.to_owned(),
            input,
            token: None,
            credentials: None,
            timeout_secs: None,
            heartbeat_secs: None,
        };
        Self::push_dispatch(step, d);
    }

    /// An item's execution ended: the frame that stands for it ends the same way.
    pub(crate) fn item_ended(&mut self, cx: Cx<'_>, f: &Frame, outcome: Outcome, step: &mut Step) {
        let Some(fm) = self.frame_mut(f.id) else {
            return;
        };
        match outcome {
            Outcome::Done(v) => {
                fm.status = Status::Done;
                fm.output = Some(text(&v));
            }
            Outcome::Failed(failure) => {
                fm.status = Status::Failed;
                fm.failure = Some(failure);
            }
            Outcome::Submitted(_) => return,
        }
        self.frame_ended(cx, f.id, step);
    }

    /// `UpdateMapRun`: a distributed Map's concurrency and tolerance change while it runs.
    pub fn update_map(
        &mut self,
        def: &crate::def::Definition,
        frame: u32,
        limit: Option<u32>,
        tolerated: (Option<f64>, Option<f64>),
        env: &Env,
    ) -> Step {
        let mut step = Step::default();
        if let Some(f) = self.frame_mut(frame) {
            if let Some(l) = limit {
                f.limit = l;
            }
            let (count, pct) = f.tolerated.unwrap_or((0.0, 0.0));
            f.tolerated = Some((tolerated.0.unwrap_or(count), tolerated.1.unwrap_or(pct)));
        }
        let cx = Cx { def, env };
        self.joined(cx, frame, 0, &mut step);
        self.pump(cx, &mut step);
        step
    }

    /// The items of a Map frame's current fan-out by status: `(pending, running, succeeded, failed, aborted, total)`.
    #[must_use]
    pub fn map_counts(&self, frame: u32) -> [u64; 6] {
        let Some(f) = self.frame(frame) else {
            return [0; 6];
        };
        let mut c = [0u64; 6];
        for k in self
            .frames
            .iter()
            .filter(|k| k.parent == frame && k.generation == f.fanouts)
        {
            let slot = match k.status {
                Status::Pending => 0,
                Status::Done => 2,
                Status::Failed => 3,
                Status::Aborted => 4,
                _ => 1,
            };
            for i in [slot, 5] {
                if let Some(x) = c.get_mut(i) {
                    *x += 1;
                }
            }
        }
        c
    }
}
