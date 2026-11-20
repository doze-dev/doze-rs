//! Pidfiles for what the engine started, and the sweep that kills what a previous daemon left behind (Go's
//! `writePidfile` and `Reconcile`). On Linux and Windows the system already takes children down with the daemon
//! (PDEATHSIG, the Job object); macOS has neither, so a daemon that was killed leaves its engines, and the next one
//! kills them by their recorded pids before it binds anything.
//!
//! Format: `run/backend-<name>.pid` holds the first process's pid as decimal digits (Go's), then a line with when it
//! started (Unix seconds, as the system reports it right after the spawn: the sweep compares like with like, so a wall
//! clock step between spawn and sweep moves both). Everything else of that instance is in the folder `run/backend-<name>.pids/`: the
//! plan's other processes as `<n>.pid`, and a short command running now (a step, a hook, an exec probe) as
//! `run-<pid>.pid`, there while it runs. A folder cannot be mistaken for another instance's file (`backend-db.1.pid`
//! once meant both instance `db`'s second process and an instance named `db.1`). Go's sweep does not parse a file
//! with the second line, so it kills nothing it cannot identify.
//!
//! The sweep kills a recorded pid only when it is still the recorded process: its start time matches the recorded one
//! (a pid the system gave to someone else since is left alone), or its leader is gone but the process group it led
//! still has members (a group id is never reused while the group exists). A Go daemon's record has no start time; its
//! process is taken as the recorded one when it leads its own group and started within seconds before the file was
//! written (Go wrote it right after the spawn), so a Go daemon's engines are swept at the cut-over too.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Where one instance's pids are recorded.
#[derive(Debug, Clone)]
pub struct Pids {
    run_dir: PathBuf,
    name: String,
}

/// A short command's record: removed when this is dropped (the command ended, or whoever ran it gave up and killed it).
#[derive(Debug)]
pub struct Recorded(PathBuf);

impl Drop for Recorded {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// When `pid` started, as the sweep will read it (now, where the system cannot say).
fn started_secs(pid: u32) -> u64 {
    doze_os::process::started_at(pid).map_or_else(|| secs(SystemTime::now()), secs)
}

impl Pids {
    /// The records of instance `name` in `run_dir`.
    pub fn new(run_dir: &Path, name: &str) -> Pids {
        Pids {
            run_dir: run_dir.into(),
            name: name.into(),
        }
    }

    fn first(&self) -> PathBuf {
        self.run_dir.join(format!("backend-{}.pid", self.name))
    }

    fn folder(&self) -> PathBuf {
        self.run_dir.join(format!("backend-{}.pids", self.name))
    }

    /// Records the pids of the instance's processes, each with its spawn time; a pid already recorded keeps its first
    /// record.
    pub fn write(&self, pids: &[u32]) {
        for (n, pid) in pids.iter().enumerate() {
            let file = if n == 0 {
                self.first()
            } else {
                self.folder().join(format!("{n}.pid"))
            };
            if read(&file).is_some_and(|(p, _)| p == *pid) {
                continue;
            }
            if let Err(e) = write_one(&file, *pid, started_secs(*pid)) {
                tracing::warn!("could not record the pid of \"{}\": {e}", self.name);
            }
        }
    }

    /// Records a short command while it runs.
    pub fn short(&self, pid: u32) -> Recorded {
        let file = self.folder().join(format!("run-{pid}.pid"));
        if let Err(e) = write_one(&file, pid, started_secs(pid)) {
            tracing::warn!("could not record a step of \"{}\": {e}", self.name);
        }
        Recorded(file)
    }

    /// Forgets the instance's processes (a short command still running keeps its own record).
    pub fn remove(&self) {
        let _ = std::fs::remove_file(self.first());
        if let Ok(entries) = std::fs::read_dir(self.folder()) {
            for e in entries.flatten() {
                let file = e.file_name().to_string_lossy().into_owned();
                let n = file.strip_suffix(".pid").unwrap_or_default();
                if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        // Only if nothing is left in it.
        let _ = std::fs::remove_dir(self.folder());
    }
}

fn write_one(file: &Path, pid: u32, now: u64) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, format!("{pid}\n{now}\n"))
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

/// Kills every recorded process that is still alive, by its exact pid (and its process group), and removes the
/// records. Returns the pids it killed.
pub fn sweep(run_dir: &Path) -> Vec<u32> {
    let mut killed = Vec::new();
    let Ok(entries) = std::fs::read_dir(run_dir) else {
        return killed;
    };
    for e in entries.flatten() {
        let file = e.file_name().to_string_lossy().into_owned();
        if !file.starts_with("backend-") {
            continue;
        }
        if file.ends_with(".pids") && e.path().is_dir() {
            if let Ok(inner) = std::fs::read_dir(e.path()) {
                for i in inner.flatten() {
                    sweep_one(&i.path(), &mut killed);
                }
            }
            let _ = std::fs::remove_dir(e.path());
        } else if file.ends_with(".pid") {
            sweep_one(&e.path(), &mut killed);
        }
    }
    killed
}

fn sweep_one(file: &Path, killed: &mut Vec<u32>) {
    if file.is_dir() || file.extension().is_none_or(|x| x != "pid") {
        return;
    }
    let sweep = doze_os::process::sweep_needed();
    let written = std::fs::metadata(file)
        .and_then(|m| m.modified())
        .map_or(0, secs);
    if let Some(pid) = read(file)
        .filter(|r| sweep && still_ours(*r, written))
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
    let _ = std::fs::remove_file(file);
}

/// How far a process's start may be from its recorded spawn (seconds: `ps` and `/proc` round).
const SAME_START: u64 = 5;

/// How long after its spawn Go may have written a record: the longest readiness budget, with its hooks.
const GO_RECORD_AFTER: u64 = 15 * 60;

/// Whether a recorded pid still names what was recorded, `written` being when its file was (Unix seconds). See the
/// module docs.
fn still_ours((pid, spawned): (u32, Option<u64>), written: u64) -> bool {
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
        // Go's record: written once the start was ready (up to its readiness budget and hooks after the spawn), by a
        // daemon whose trees led their own groups (audit 3, minor 3).
        (Some(a), None) => {
            a <= written + 1
                && written - a.min(written) <= GO_RECORD_AFTER
                && doze_os::process::leads_own_group(pid)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_remove_and_sweep_dead_pids() {
        let d = std::env::temp_dir().join(format!("doze-engine-pid-{}", std::process::id()));
        let db = Pids::new(&d, "db");
        // An instance whose name looks like another's numbered file.
        let dot = Pids::new(&d, "db.1");
        db.write(&[1_999_999_000, 1_999_999_001]);
        dot.write(&[1_999_999_002]);
        let text = std::fs::read_to_string(d.join("backend-db.pid")).unwrap();
        assert!(text.starts_with("1999999000\n"), "{text:?}");
        assert!(d.join("backend-db.pids/1.pid").exists());
        let step = db.short(1_999_999_003);
        assert!(d.join("backend-db.pids/run-1999999003.pid").exists());
        db.remove();
        assert!(!d.join("backend-db.pid").exists() && !d.join("backend-db.pids/1.pid").exists());
        assert!(
            d.join("backend-db.pids/run-1999999003.pid").exists(),
            "a step still running keeps its record"
        );
        drop(step);
        assert!(!d.join("backend-db.pids/run-1999999003.pid").exists());
        assert!(
            d.join("backend-db.1.pid").exists(),
            "another instance's file stays"
        );
        let _left = db.short(1_999_999_004);
        // Not alive: nothing killed, the files and the folder go.
        assert!(sweep(&d).is_empty());
        assert!(!d.join("backend-db.1.pid").exists());
        assert!(!d.join("backend-db.pids").exists());
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
            assert!(still_ours((me, Some(started)), now), "its own start time");
            assert!(
                !still_ours((me, Some(now + 3600)), now),
                "a pid reused by another process"
            );
            assert!(
                !still_ours((me, None), now + 3600),
                "a Go record written long after this process started names another process"
            );
        }
    }
}
