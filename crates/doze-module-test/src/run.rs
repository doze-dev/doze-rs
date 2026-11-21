//! Running a module's plan the way the core does, with the core's own executor (`doze_engine::exec`, promoted from
//! here in E1): processes in `after` order as doze-os trees, each gated on its readiness check, then its hooks; stopped
//! in reverse order with the module's declared stop. Output goes to one run of the log store, as the core keeps it.

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

/// How long a process that started but failed the boot is given to stop the way its module declared (a killed
/// PostgreSQL leaves its System V shared memory segment behind; ci-health.md, "Landing core M1").
pub(crate) const FAILED_BOOT_GRACE: Duration = Duration::from_secs(30);

/// Starts every process of `plan`, each ready (within `limit`) before the next, running hooks after each. On a
/// failure `exec::start` has already stopped the processes it started, each the way its module declared.
pub async fn start(plan: &Plan, logs: &Path, limit: Duration) -> Result<Started, String> {
    let log = RunLog::open(logs, "plan").map_err(|e| e.to_string())?;
    let unit = exec::start(plan, &log, limit, None)
        .await
        .map_err(|e| e.message)?;
    Ok(Started { unit, log })
}

impl Started {
    /// What the plan's processes printed so far (one log for the plan, as the core keeps it).
    pub fn log(&self, _process: &str) -> String {
        self.log.text()
    }

    /// Stops every process in reverse order, each the way its module declared; how each ended. A stop that was not
    /// graceful prints the plan's log: what the engine said while it would not stop is the only evidence of why, and
    /// the sandbox holding it is gone with the test (ferret's postgres, forced on macOS CI on 2026-11-20, left none).
    pub async fn stop(mut self, grace: Duration) -> Vec<(String, std::io::Result<Stopped>)> {
        let stops = self.unit.stop(grace).await;
        if let Some(report) = not_graceful(&stops, &self.log.text()) {
            eprintln!("{report}");
        }
        stops
    }
}

/// The most lines of the plan's log a stop that was not graceful quotes (its end: the stop is there).
const QUOTED: usize = 200;

/// For stops that were not all graceful successes, what they were and the end of the plan's log.
fn not_graceful(stops: &[(String, std::io::Result<Stopped>)], log: &str) -> Option<String> {
    let clean = stops
        .iter()
        .all(|(_, r)| matches!(r, Ok(Stopped::Graceful(st)) if st.success()));
    if clean {
        return None;
    }
    let lines: Vec<&str> = log.lines().collect();
    let tail = lines
        .get(lines.len().saturating_sub(QUOTED)..)
        .unwrap_or_default();
    Some(format!(
        "not every process stopped gracefully: {stops:?}\nthe plan's log, its last {} lines:\n{}",
        tail.len(),
        tail.join("\n")
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    #[test]
    fn only_a_stop_that_was_not_graceful_quotes_the_end_of_the_log() {
        let ok = |code| Ok(Stopped::Graceful(ExitStatus::from_raw(code)));
        let log: String = (1..=250).map(|n| format!("line {n}\n")).collect();
        assert_eq!(not_graceful(&[("a".into(), ok(0))], &log), None);
        let forced = [
            ("ferretdb".into(), ok(0)),
            (
                "postgres".into(),
                Ok(Stopped::Forced(ExitStatus::from_raw(9))),
            ),
        ];
        let report = not_graceful(&forced, &log).unwrap();
        assert!(report.contains("Forced"), "{report}");
        assert!(report.contains("its last 200 lines"), "{report}");
        assert!(report.ends_with("line 250") && report.contains("\nline 51\n"));
        assert!(!report.contains("\nline 50\n"), "{report}");
        // A graceful exit that failed is reported too.
        assert!(not_graceful(&[("a".into(), ok(256))], "x").is_some());
    }
}
