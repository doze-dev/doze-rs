//! The machine's own hands: the `doze` CLI run as the person with the system's prompt in place of sudo, the
//! administrator prompt itself (`doze_os::privilege::admin`), processes by exact pid, folders.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::repair::{Fail, Hands, PROMPT_ENV, doze_outcome};

/// The real machine. Only the app makes one, and only on a person's click.
#[derive(Debug, Clone)]
pub struct SystemHands {
    /// The `doze` to run.
    pub doze: PathBuf,
    /// Where a doze command with no stack runs.
    pub home: PathBuf,
}

impl Hands for SystemHands {
    fn doze(&mut self, dir: Option<&Path>, args: &[String]) -> Result<(), Fail> {
        let out = Command::new(&self.doze)
            .args(args)
            .current_dir(dir.unwrap_or(&self.home))
            .env("NO_COLOR", "1")
            .env(PROMPT_ENV, "1")
            .env_remove("DOZE_APP_CHILD")
            .output()
            .map_err(|e| Fail::Failed(format!("could not run doze: {e}")))?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        doze_outcome(out.status.success(), &text)
    }

    fn admin(&mut self, what: &str, script: &str) -> Result<(), Fail> {
        doze_os::privilege::admin(what, script).map_err(|e| match e {
            doze_os::privilege::AdminError::Cancelled => Fail::Cancelled,
            other => Fail::Failed(other.to_string()),
        })
    }

    fn terminate(&mut self, pid: u32) -> Result<(), Fail> {
        doze_os::process::terminate(pid).map_err(|e| Fail::Failed(e.to_string()))
    }

    fn kill(&mut self, pid: u32) -> Result<(), Fail> {
        doze_os::process::kill(pid).map_err(|e| Fail::Failed(e.to_string()))
    }

    fn alive(&mut self, pid: u32) -> bool {
        doze_os::process::alive(pid)
    }

    fn pause(&mut self) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    fn remove_dir(&mut self, dir: &Path) -> Result<(), Fail> {
        std::fs::remove_dir_all(dir).map_err(|e| Fail::Failed(e.to_string()))
    }
}
