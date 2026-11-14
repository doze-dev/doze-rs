//! Small reads and writes of what the engine holds per instance, kept out of the start and stop sequences.

use crate::engine::Engine;
use crate::logs::RunLog;
use crate::process::{ProcessSpec, Step};
use crate::stack::{Endpoint, PlanCtx};
use crate::steps::StepEnv;

impl Engine {
    /// Where `name` was last reached (a `Lazy` need is wired without waking it).
    pub(crate) fn live_endpoint(&self, name: &str) -> Endpoint {
        self.update(|c| {
            c.live
                .get(name)
                .map(|l| l.endpoint.clone())
                .unwrap_or_default()
        })
    }

    /// Keeps a command's `on_ready` and `stop` steps, with where they run, for this start.
    pub(crate) fn remember_steps(
        &self,
        name: &str,
        spec: &ProcessSpec,
        ctx: &PlanCtx,
        private: Option<u16>,
    ) {
        let at = StepEnv {
            dir: spec.dir.clone(),
            env: spec.env_for(ctx, private),
        };
        let steps = (spec.on_ready.clone(), spec.stop.clone(), at);
        self.update(|c| c.live.entry(name.into()).or_default().steps = Some(steps));
    }

    /// The `on_ready` steps of `name`, if it is a command.
    pub(crate) fn on_ready_steps(&self, name: &str) -> Option<(Vec<Step>, StepEnv)> {
        self.update(|c| {
            let (on_ready, _, at) = c.live.get(name)?.steps.as_ref()?;
            Some((on_ready.clone(), at.clone()))
        })
    }

    /// The `stop` steps of `name` and its log, if it is a command that has some.
    pub(crate) fn stop_steps(&self, name: &str) -> Option<(Vec<Step>, StepEnv, RunLog)> {
        self.update(|c| {
            let live = c.live.get(name)?;
            let (_, stop, at) = live.steps.as_ref()?;
            if stop.is_empty() {
                return None;
            }
            Some((stop.clone(), at.clone(), live.log.clone()?))
        })
    }

    /// For a start that only has to run `on_ready` again: its log, pid and backend, when it is still running.
    pub(crate) fn pending_ready(&self, name: &str) -> Option<(RunLog, u32, String)> {
        self.update(|c| {
            let live = c.live.get(name)?;
            let (_, unit) = live.unit.as_ref()?;
            if !live.ready_pending || !unit.alive() {
                return None;
            }
            Some((live.log.clone()?, unit.pid(), live.endpoint.backend.clone()))
        })
    }
}
