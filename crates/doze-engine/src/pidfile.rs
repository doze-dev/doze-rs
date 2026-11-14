//! Pidfiles for what the engine started, and the sweep that kills what a previous daemon left behind (Go's
//! `writePidfile` and `Reconcile`). On Linux and Windows the system already takes children down with the daemon
//! (PDEATHSIG, the Job object); macOS has neither, so a daemon that was killed leaves its engines, and the next one
//! kills them by their recorded pids before it binds anything.
//!
//! Format (Go's): `run/backend-<name>.pid` holds the first process's pid as decimal digits. A plan of several processes
//! also writes `run/backend-<name>.<n>.pid` for the others, which Go's sweep reads the same way.

use std::path::{Path, PathBuf};

fn path(run_dir: &Path, name: &str, n: usize) -> PathBuf {
    if n == 0 {
        run_dir.join(format!("backend-{name}.pid"))
    } else {
        run_dir.join(format!("backend-{name}.{n}.pid"))
    }
}

/// Records the pids of `name`'s processes.
pub fn write(run_dir: &Path, name: &str, pids: &[u32]) {
    let _ = std::fs::create_dir_all(run_dir);
    for (n, pid) in pids.iter().enumerate() {
        if let Err(e) = std::fs::write(path(run_dir, name, n), pid.to_string()) {
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
        let pid = std::fs::read_to_string(e.path())
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .filter(|p| *p > 0);
        let sweep = doze_os::process::sweep_needed();
        if let Some(pid) = pid.filter(|p| sweep && doze_os::process::tree_alive(*p)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_remove_and_sweep_dead_pids() {
        let d = std::env::temp_dir().join(format!("doze-engine-pid-{}", std::process::id()));
        write(&d, "db", &[1_999_999_000, 1_999_999_001]);
        write(&d, "db2", &[1_999_999_002]);
        assert_eq!(
            std::fs::read_to_string(d.join("backend-db.pid")).unwrap(),
            "1999999000"
        );
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
}
