//! What the real-process tests share: a private data folder per test, the helper binary, and the "nothing left behind"
//! check by exact pid.
#![allow(dead_code, unreachable_pub)]

use doze_engine::{Engine, ProcessSpec, Restart, Service};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const HELPER: &str = env!("CARGO_BIN_EXE_doze-engine-helper");

/// A fresh folder for one test (removed by [`Dir`]'s drop).
pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new(tag: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("dze-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        assert!(std::fs::create_dir_all(&p).is_ok(), "{p:?}");
        Dir(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The helper as a run line.
pub fn run(args: &[&str]) -> Vec<String> {
    std::iter::once(HELPER.to_string())
        .chain(args.iter().map(|s| (*s).to_string()))
        .collect()
}

/// A fronted helper that serves HTTP once it has started (after `delay_ms`).
pub fn listener(dir: &Dir, delay_ms: u64) -> ProcessSpec {
    ProcessSpec::new(run(&["listen", &delay_ms.to_string()]), dir.path())
        .port(1)
        .env("STARTS", &dir.file("starts").to_string_lossy())
}

/// A service running the helper.
pub fn svc(name: &str, spec: ProcessSpec) -> Service {
    Service::process(name, spec)
}

/// A quick restart policy for tests.
pub fn quick_restart(max: u32) -> Restart {
    Restart {
        policy: doze_engine::Policy::OnFailure,
        backoff: Duration::from_millis(100),
        max_retries: max,
    }
}

/// Lines in a file (0 when missing).
pub fn lines(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// Waits until `f` holds or `limit` passes; whether it held.
pub async fn eventually(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < limit {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    f()
}

/// The ps check: none of these pids (nor a group they led) is left, within a few seconds.
pub async fn all_gone(pids: &[u32]) -> bool {
    eventually(Duration::from_secs(5), || {
        pids.iter().all(|p| !doze_os::process::tree_alive(*p))
    })
    .await
}

/// A pid from a file the helper wrote.
pub fn pid_in(path: &Path) -> u32 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// The pid of a running instance.
pub fn pid_of(e: &Engine, name: &str) -> u32 {
    e.get(name).map_or(0, |i| i.pid)
}
