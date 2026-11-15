//! Taking a stack out of doze (Go `serve.remove`), running or not, whatever is left of its folder: its daemon is
//! stopped by its pid (so a deleted folder is no obstacle), its record and stale control files go. Its folder, doze.hcl
//! and data stay where they are; opening the folder again brings it back, with its data.
//!
//! Go also forgets the domain routes the stack claimed; that is the names daemon's, and waits for its Rust port: a
//! route left behind shows in Settings › Machine as left behind, and `doze doctor` names it.

use std::time::{Duration, Instant};

use super::failure::Failure;
use crate::StackKey;

/// How long a daemon may take to stop (what `doze down` gives it).
pub const STOP_WAIT: Duration = Duration::from_secs(20);

/// Removes the stack `key`, named `name`. Blocks while its daemon stops.
pub fn remove(key: &StackKey, name: &str) -> Result<String, Failure> {
    if let Some(d) = doze_projects::daemon_pid(key).filter(|d| d.alive) {
        // The daemon stops its services as it exits.
        let _ = doze_os::process::terminate(d.pid);
        let start = Instant::now();
        while doze_os::process::alive(d.pid) && start.elapsed() < STOP_WAIT {
            std::thread::sleep(Duration::from_millis(100));
        }
        if doze_os::process::alive(d.pid) {
            return Err(Failure {
                what: format!("Removing {name} failed"),
                lines: vec![format!(
                    "its daemon (pid {}) did not stop; try doze down in its folder",
                    d.pid
                )],
                problem: format!("its daemon (pid {}) did not stop", d.pid),
                config: None,
            });
        }
    }
    for f in [key.record(), key.socket(), key.pid_file()] {
        let _ = std::fs::remove_file(f);
    }
    Ok(format!(
        "removed {name} from doze; its folder and data are still on disk, and opening the folder brings it back"
    ))
}

#[cfg(test)]
mod tests {
    use super::remove;
    use crate::StackKey;

    #[test]
    fn a_stopped_stack_loses_its_record_and_keeps_its_folder() {
        let home = tempfile::tempdir().unwrap();
        let key = StackKey::new(home.path().join("projects/acme-1"));
        std::fs::create_dir_all(key.run_dir()).unwrap();
        std::fs::write(key.record(), "{}").unwrap();
        std::fs::write(key.pid_file(), "999999999\n").unwrap();
        let said = remove(&key, "acme").unwrap();
        assert!(said.starts_with("removed acme from doze"));
        assert!(!key.record().exists() && !key.pid_file().exists());
        assert!(key.dir().exists());
    }
}
