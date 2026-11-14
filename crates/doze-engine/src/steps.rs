//! The steps around a command's run (Go's `runtime/steps.go`, lifecycle.md): `setup` once per data folder, `on_ready`
//! once it is ready, `stop` in place of the signals. Each is argument words, no shell (PLAN.md D14); its output goes to
//! the instance's log under a step tag, and each is a journal event with how long it took.

use crate::exec::program_of;
use crate::logs::RunLog;
use crate::process::Step;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// The file in the data folder that says setup ran, holding a hash of the steps it ran (Go's on-disk format).
pub const SETUP_MARKER: &str = ".doze-setup";

/// How long a command gets to exit after its `stop` steps ran, before the signals.
pub const STOP_PATIENCE: Duration = Duration::from_secs(10);

/// The moment a step runs at: `setup`, `on_ready`, `stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    Setup,
    OnReady,
    Stop,
}

impl Moment {
    pub fn as_str(self) -> &'static str {
        match self {
            Moment::Setup => "setup",
            Moment::OnReady => "on_ready",
            Moment::Stop => "stop",
        }
    }
}

/// Where and with what steps run: the command's own folder and environment.
#[derive(Debug, Clone)]
pub struct StepEnv {
    pub dir: PathBuf,
    pub env: BTreeMap<String, String>,
}

/// One step that ran: what the journal records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub took: Duration,
    /// `setup: <line>`, with ` — <error>` when it failed.
    pub detail: String,
}

fn setup_hash(steps: &[Step]) -> String {
    let lines: Vec<&str> = steps.iter().map(|s| s.line.as_str()).collect();
    hex::encode(Sha256::digest(lines.join("\n").as_bytes()))
}

/// Whether setup has to run for this data folder: there are steps, and the marker is missing or names other steps.
pub fn setup_needed(data_dir: &Path, steps: &[Step]) -> bool {
    if steps.is_empty() {
        return false;
    }
    let have = std::fs::read_to_string(data_dir.join(SETUP_MARKER)).unwrap_or_default();
    have.trim() != setup_hash(steps)
}

/// Records that setup ran for these steps.
pub fn mark_setup_done(data_dir: &Path, steps: &[Step]) -> std::io::Result<()> {
    std::fs::write(
        data_dir.join(SETUP_MARKER),
        format!("{}\n", setup_hash(steps)),
    )
}

/// Runs `steps` one after another, stopping at the first failure. `ran` hears about each.
pub async fn run(
    moment: Moment,
    steps: &[Step],
    at: &StepEnv,
    log: &RunLog,
    mut ran: impl FnMut(Ran),
) -> Result<(), String> {
    for step in steps {
        if step.line.trim().is_empty() {
            continue;
        }
        let started = Instant::now();
        let res = run_one(step, at, log).await;
        let mut detail = format!("{}: {}", moment.as_str(), step.line);
        if let Err(e) = &res {
            detail.push_str(" — ");
            detail.push_str(e);
        }
        ran(Ran {
            took: started.elapsed(),
            detail,
        });
        if let Err(e) = res {
            return Err(format!(
                "{} step \"{}\" failed: {e}",
                moment.as_str(),
                step.line
            ));
        }
    }
    Ok(())
}

async fn run_one(step: &Step, at: &StepEnv, log: &RunLog) -> Result<(), String> {
    let (prog, args) = step.argv.split_first().ok_or("an empty step")?;
    let st = tokio::process::Command::new(program_of(prog))
        .args(args)
        .envs(&at.env)
        .current_dir(&at.dir)
        .stdin(Stdio::null())
        .stdout(log.stdio().map_err(|e| e.to_string())?)
        .stderr(log.stdio().map_err(|e| e.to_string())?)
        .kill_on_drop(true)
        .status()
        .await
        .map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err(doze_os::process::exit_text(&st))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_holds_gos_hash_of_the_lines() {
        let d = std::env::temp_dir().join(format!("doze-engine-steps-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let steps = vec![Step::argv(["bun", "install"]), Step::argv(["make", "db"])];
        assert!(setup_needed(&d, &steps));
        assert!(!setup_needed(&d, &[]));
        mark_setup_done(&d, &steps).unwrap();
        assert!(!setup_needed(&d, &steps));
        // Go: sha256 of the lines joined by "\n", hex, then a newline.
        let want = hex::encode(Sha256::digest(b"bun install\nmake db"));
        assert_eq!(
            std::fs::read_to_string(d.join(SETUP_MARKER)).unwrap(),
            format!("{want}\n")
        );
        assert!(setup_needed(&d, &steps[..1]), "other steps run again");
        let _ = std::fs::remove_dir_all(&d);
    }
}
