//! A distributed Map's items, run as executions of their own (as AWS runs them, so they are listed with the Map Run's ARN). Each item
//! is a frame that only stands for its execution: when the concurrency allows, the frame is "called" with the item processor's
//! definition and input, the service starts the execution, and the execution's end is the frame's answer. The join, the tolerance,
//! the counts and the result writer are the Map's as for any branch.

use serde_json::{Value, json};

use super::io::text;
use super::{Cx, Dispatch, Exec, Frame, Outcome, Status, Step};
use crate::def::{Kind, State};

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
}
