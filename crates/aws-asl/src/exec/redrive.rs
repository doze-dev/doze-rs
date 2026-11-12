//! Redriving a failed, aborted or timed-out execution: it carries on from where it stopped. A branch that failed runs its state
//! again; a Parallel or Map state that failed because of a branch waits for its branches again, and only those that did not succeed
//! run again (the ones that did keep their output).

use serde_json::json;

use super::{Cx, Exec, Status, Step, sub};
use crate::def::{Definition, Kind};
use crate::env::Env;

impl Exec {
    /// Makes the execution runnable again and records `ExecutionRedriven`; the caller then runs it.
    pub fn redrive(&mut self, def: &Definition, env: &Env) -> Step {
        let mut step = Step::default();
        self.redrive_count += 1;
        if self.deadline > 0 {
            // (memory) The machine's TimeoutSeconds counts again from the redrive.
            let span = (self.deadline - self.start_ms).max(0);
            self.deadline = env.now_ms + span;
        }
        let cx = Cx { def, env };
        self.again(cx, 1);
        let count = self.redrive_count;
        self.recorder(&mut step, env).add(
            1,
            "ExecutionRedriven",
            Some((
                "executionRedrivenEventDetails",
                json!({"redriveCount": count}),
            )),
        );
        // A Map whose held-back items are all that is left starts them again.
        let joins: Vec<u32> = self
            .frames
            .iter()
            .filter(|f| f.status == Status::Join)
            .map(|f| f.id)
            .collect();
        for id in joins {
            self.joined(cx, id, 0, &mut step);
        }
        step
    }

    fn again(&mut self, cx: Cx<'_>, id: u32) {
        let Some(f) = self.frame(id).cloned() else {
            return;
        };
        if !matches!(f.status, Status::Failed | Status::Aborted) {
            return;
        }
        let fan = sub(cx.def, &f.def)
            .and_then(|d| d.state(&f.state))
            .is_some_and(|s| matches!(s.kind, Kind::Parallel { .. } | Kind::Map(_)));
        let kids: Vec<(u32, Status)> = self
            .frames
            .iter()
            .filter(|k| k.parent == id && k.generation == f.fanouts)
            .map(|k| (k.id, k.status))
            .collect();
        let rejoin = fan && !kids.is_empty() && kids.iter().any(|(_, s)| *s != Status::Done);
        if let Some(fm) = self.frame_mut(id) {
            fm.failure = None;
            fm.attempts.clear();
            fm.status = if rejoin {
                Status::Join
            } else {
                Status::Runnable
            };
        }
        if rejoin {
            for (kid, status) in kids {
                if status == Status::Aborted
                    && self.frame(kid).is_some_and(|k| k.entered_event == 0)
                {
                    if let Some(k) = self.frame_mut(kid) {
                        k.status = Status::Pending;
                    }
                    continue;
                }
                self.again(cx, kid);
            }
        }
    }
}
