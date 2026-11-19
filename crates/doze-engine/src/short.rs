//! Short commands (steps, hooks, exec probes) as doze-os trees (audit B3): each is recorded in the instance's pidfiles
//! while it runs, bounded by a limit, and taken down with everything it started when it ends, overruns, or whoever ran
//! it gives up. On Linux and Windows a daemon that dies takes it down too (PDEATHSIG, the Job object); on macOS the next
//! daemon's sweep finds its record.
//!
//! One deliberate difference from Go, whose `sh -c` steps could leave a background process running: whatever a short
//! command started is killed when it ends, so "nothing left behind" holds for steps as for processes.

use crate::logs::{Capture, RunLog};
use crate::pidfile::Pids;
use doze_os::tree::Tree;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// One short command: argument words (no shell, PLAN.md D14) and where it runs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Short<'a> {
    pub argv: &'a [String],
    /// Added to doze's own environment; its `PATH`, if any, is where the program is looked up.
    pub env: Option<&'a BTreeMap<String, String>>,
    pub dir: Option<&'a Path>,
    /// Where its output goes, on which stream (nowhere when `None`).
    pub out: Option<(&'a RunLog, &'a str)>,
    /// Also keeps the end of its output, to quote (a hook's failure).
    pub capture: Option<&'a Capture>,
}

/// The program to run: a bare name is looked up on the `PATH` the command is given (else doze's own), with `PATHEXT`
/// on Windows, without a shell.
pub(crate) fn program_in(name: &str, env: Option<&BTreeMap<String, String>>) -> PathBuf {
    let bare = !name.contains('/') && !name.contains('\\');
    if !bare {
        return name.into();
    }
    let found = match env.and_then(|e| e.get("PATH")) {
        Some(path) => {
            let pathext = env
                .and_then(|e| e.get("PATHEXT").map(std::ffi::OsString::from))
                .or_else(|| std::env::var_os("PATHEXT"));
            doze_os::which::which_in(name, path.as_ref(), pathext.as_deref())
        }
        None => doze_os::which::which(name),
    };
    found.unwrap_or_else(|| name.into())
}

/// Runs `s` to its end within `limit`. `Err` says why it failed, in Go's words ("exit status 1", "signal: killed").
pub(crate) async fn run(s: Short<'_>, limit: Duration, pids: Option<&Pids>) -> Result<(), String> {
    let (prog, args) = s.argv.split_first().ok_or("an empty command")?;
    let mut cmd = tokio::process::Command::new(program_in(prog, s.env));
    cmd.args(args).stdin(Stdio::null());
    if let Some(env) = s.env {
        cmd.envs(env);
    }
    if let Some(d) = s.dir {
        cmd.current_dir(d);
    }
    if s.out.is_some() {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let mut tree = Tree::spawn(cmd).map_err(|e| e.to_string())?;
    let _recorded = pids.and_then(|p| tree.pid().map(|pid| p.short(pid)));
    let pumps = match s.out {
        Some((log, stream)) => {
            let own = Capture::default();
            log.attach_short(&mut tree, stream, s.capture.unwrap_or(&own))
        }
        None => Vec::new(),
    };
    let ended = wait(&mut tree, limit).await;
    // The tree is gone (its pipes closed with it): what it printed last reaches the log before this returns.
    crate::logs::drain(pumps).await;
    ended
}

async fn wait(tree: &mut Tree, limit: Duration) -> Result<(), String> {
    // Whatever it started goes with it (`wait` kills the rest of its tree); after an overrun, so does the command.
    match tokio::time::timeout(limit, tree.wait()).await {
        Ok(Ok(st)) if st.success() => Ok(()),
        Ok(Ok(st)) => Err(doze_os::process::exit_error(&st)),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => {
            let _ = tree.kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), tree.wait()).await;
            Err(format!(
                "did not finish within {}",
                crate::godur::go_duration(limit)
            ))
        }
    }
}
