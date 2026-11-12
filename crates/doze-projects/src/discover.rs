//! Finding the running stacks, as the Go app finds them: every control socket under `<home>/projects/*/run/`, less
//! those whose pid file names a process that is gone (a socket that outlived its daemon, after a kill or a reboot:
//! that stack is stopped, not "not answering").

use crate::paths::{StackKey, project_dirs};
use std::path::{Path, PathBuf};

/// A stack with a control socket on disk and no sign its daemon is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    /// The stack.
    pub key: StackKey,
    /// Its control socket.
    pub socket: PathBuf,
    /// Its daemon's pid, when the pid file names one (a fixture's stack has none).
    pub pid: Option<u32>,
}

/// What a stack's pid file says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaemonPid {
    /// The pid it names.
    pub pid: u32,
    /// Whether a process with that pid exists.
    pub alive: bool,
}

/// What the pid file in `key` says. `None` when there is none, or it names no pid: nothing can be said.
pub fn daemon_pid(key: &StackKey) -> Option<DaemonPid> {
    let raw = std::fs::read_to_string(key.pid_file()).ok()?;
    let first = raw.split('\n').next().unwrap_or_default().trim();
    let pid: i64 = first.parse().ok().filter(|&p| p > 0)?;
    // A pid past what a system hands out names no process.
    Some(match u32::try_from(pid) {
        Ok(pid) => DaemonPid {
            pid,
            alive: doze_os::process::alive(pid),
        },
        Err(_) => DaemonPid {
            pid: u32::MAX,
            alive: false,
        },
    })
}

/// The stacks running under `home`, in folder-name order.
pub fn running(home: &Path) -> Vec<Running> {
    project_dirs(home)
        .into_iter()
        .map(StackKey::new)
        .filter(|k| k.socket().symlink_metadata().is_ok())
        .filter_map(|key| {
            let pid = daemon_pid(&key);
            if pid.is_some_and(|p| !p.alive) {
                return None;
            }
            Some(Running {
                socket: key.socket(),
                pid: pid.map(|p| p.pid),
                key,
            })
        })
        .collect()
}
