//! Pidfiles for what the engine started, and the sweep that kills what a previous daemon left behind (Go's
//! `writePidfile` and `Reconcile`). On Linux and Windows the system already takes children down with the daemon
//! (PDEATHSIG, the Job object); macOS has neither, so a daemon that was killed leaves its engines, and the next one
//! kills them by their recorded pids before it binds anything.
//!
//! Format: `run/backend-<name>.pid` holds the first process's pid as decimal digits (Go's), then a line with when it
//! was spawned (Unix seconds); a plan of several processes also writes `run/backend-<name>.<n>.pid` for the others.
//! Go's sweep does not parse a file with the second line, so it kills nothing it cannot identify.
//!
//! The sweep kills a recorded pid only when it is still the recorded process: its start time matches the spawn time
//! (a pid the system gave to someone else since is left alone), or its leader is gone but the process group it led
//! still has members (a group id is never reused while the group exists).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

fn path(run_dir: &Path, name: &str, n: usize) -> PathBuf {
    if n == 0 {
        run_dir.join(format!("backend-{name}.pid"))
    } else {
        run_dir.join(format!("backend-{name}.{n}.pid"))
    }
}

fn read(file: &Path) -> Option<(u32, Option<u64>)> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut lines = text.lines();
    let pid = lines
        .next()?
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|p| *p > 0)?;
    Some((pid, lines.next().and_then(|l| l.trim().parse().ok())))
}

/// Records the pids of `name`'s processes, each with its spawn time; a pid already recorded keeps its first record.
pub fn write(run_dir: &Path, name: &str, pids: &[u32]) {
    let _ = std::fs::create_dir_all(run_dir);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    for (n, pid) in pids.iter().enumerate() {
        let file = path(run_dir, name, n);
        if read(&file).is_some_and(|(p, _)| p == *pid) {
            continue;
        }
        if let Err(e) = std::fs::write(&file, format!("{pid}\n{now}\n")) {
            tracing::warn!("could not record the pid of \"{name}\": {e}");
        }
    }
}

/// Forgets the pids of `name`.
pub fn remove(run_dir: &Path, name: &str) {
    let _ = std::fs::remove_file(path(run_dir, name, 0));
    let prefix = format!("backend-{name}.");
    let Ok(entries) = std::fs::read_dir(run_dir) else {
        return;
    };
    for e in entries.flatten() {
        let file = e.file_name().to_string_lossy().into_owned();
        let rest = file
            .strip_prefix(&prefix)
            .and_then(|r| r.strip_suffix(".pid"));
        if rest.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Kills every recorded process that is still alive, by its exact pid (and its process group), and removes the
/// pidfiles. Returns the pids it killed.
pub fn sweep(run_dir: &Path) -> Vec<u32> {
    let mut killed = Vec::new();
    let Ok(entries) = std::fs::read_dir(run_dir) else {
        return killed;
    };
    for e in entries.flatten() {
        let file = e.file_name().to_string_lossy().into_owned();
        if !file.starts_with("backend-") || !file.ends_with(".pid") || e.path().is_dir() {
            continue;
        }
        let sweep = doze_os::process::sweep_needed();
        if let Some(pid) = read(&e.path())
            .filter(|r| sweep && still_ours(*r))
            .map(|(p, _)| p)
        {
            match doze_os::process::kill_recorded(pid) {
                Ok(()) => {
                    tracing::info!("reclaimed an orphaned backend (pid {pid}) from a previous run");
                    killed.push(pid);
                }
                Err(err) => tracing::warn!("could not reclaim pid {pid}: {err}"),
            }
        }
        let _ = std::fs::remove_file(e.path());
    }
    killed
}

/// How far a process's start may be from its recorded spawn (seconds: `ps` and `/proc` round).
const SAME_START: u64 = 5;

/// Whether a recorded pid still names what was recorded. See the module docs.
fn still_ours((pid, spawned): (u32, Option<u64>)) -> bool {
    if !doze_os::process::tree_alive(pid) {
        return false;
    }
    if !doze_os::process::alive(pid) {
        return true; // the leader is gone and its group lives: the group id cannot have been reused
    }
    let started = doze_os::process::started_at(pid)
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    match (started, spawned) {
        (Some(a), Some(b)) => a.abs_diff(b) <= SAME_START,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_remove_and_sweep_dead_pids() {
        let d = std::env::temp_dir().join(format!("doze-engine-pid-{}", std::process::id()));
        write(&d, "db", &[1_999_999_000, 1_999_999_001]);
        write(&d, "db2", &[1_999_999_002]);
        let text = std::fs::read_to_string(d.join("backend-db.pid")).unwrap();
        assert!(text.starts_with("1999999000\n"), "{text:?}");
        assert!(d.join("backend-db.1.pid").exists());
        remove(&d, "db");
        assert!(!d.join("backend-db.pid").exists() && !d.join("backend-db.1.pid").exists());
        assert!(
            d.join("backend-db2.pid").exists(),
            "another instance's file stays"
        );
        // Not alive: nothing killed, the file goes.
        assert!(sweep(&d).is_empty());
        assert!(!d.join("backend-db2.pid").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_live_pid_is_killed_only_while_it_is_the_recorded_process() {
        // This test process: alive, started well before "now" was recorded as its spawn.
        let me = std::process::id();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if doze_os::process::sweep_needed() && doze_os::process::started_at(me).is_some() {
            let started = doze_os::process::started_at(me)
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok());
            let started = started.map_or(0, |d| d.as_secs());
            assert!(still_ours((me, Some(started))), "its own start time");
            assert!(
                !still_ours((me, Some(now + 3600))),
                "a pid reused by another process"
            );
            assert!(
                !still_ours((me, None)),
                "nothing to identify it by (a Go pidfile)"
            );
        }
    }
}
