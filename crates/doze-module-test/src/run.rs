//! Running a module's plan the way the core does, with the core's own executor (`doze_engine::exec`, promoted from
//! here in E1): processes in `after` order as doze-os trees, each gated on its readiness check, then its hooks; stopped
//! in reverse order with the module's declared stop. Output goes to one log file, never pipes (W0).

use doze_engine::exec::{self, Unit};
use doze_engine::logs::RunLog;
use doze_module_proto::msg::run::Plan;
use doze_os::tree::Stopped;
use std::path::Path;
use std::time::Duration;

/// The running processes of one plan.
#[derive(Debug)]
pub struct Started {
    unit: Unit,
    log: RunLog,
}

/// Starts every process of `plan`, each ready (within `limit`) before the next, running hooks after each.
pub async fn start(plan: &Plan, logs: &Path, limit: Duration) -> Result<Started, String> {
    let log = RunLog::open(logs, "plan").map_err(|e| e.to_string())?;
    let unit = exec::start(plan, &log, limit, &mut |_| {})
        .await
        .map_err(|e| e.message)?;
    Ok(Started { unit, log })
}

impl Started {
    /// What the plan's processes printed so far (one log for the plan, as the core keeps it).
    pub fn log(&self, _process: &str) -> String {
        std::fs::read_to_string(self.log.path()).unwrap_or_default()
    }

    /// Stops every process in reverse order, each the way its module declared; how each ended.
    pub async fn stop(mut self, grace: Duration) -> Vec<(String, std::io::Result<Stopped>)> {
        self.unit.stop(grace).await
    }
}
