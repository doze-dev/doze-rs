//! The lock beside doze.hcl: the engine pins a module is handed, and the ones it reports back.

use std::path::Path;
use std::sync::Mutex;

/// Every engine pin of the lock at `path`, as the module protocol carries them (none when there is no lock).
pub fn lock_pins(path: &Path) -> Vec<doze_module_proto::msg::run::Pin> {
    let Ok(lock) = doze_fetch::lock::Lock::load(path) else {
        return Vec::new();
    };
    lock.engines
        .into_iter()
        .flat_map(|(engine, specs)| {
            specs
                .into_iter()
                .map(move |(spec, p)| doze_module_proto::msg::run::Pin {
                    engine: engine.clone(),
                    spec,
                    resolved: p.resolved,
                    hashes: p.hashes,
                })
        })
        .collect()
}

/// Records engine pins a module reported in the lock at `path` (Go's `up` wrote them), so the next start resolves
/// offline. A lock that cannot be read or written is said in the log and left alone: the start goes on.
#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
pub fn record_pins(path: &Path, pins: &[doze_module_proto::msg::run::Pin]) {
    // One writer at a time: two instances starting at once would each read, change and save, one losing the other's
    // pins (and the save's temporary file is named by the pid, so the second rename found it gone).
    static WRITING: Mutex<()> = Mutex::new(());
    let _one = crate::lock(&WRITING);
    let mut lock = if path.exists() {
        match doze_fetch::lock::Lock::load(path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("doze daemon: {} not updated: {e}", path.display());
                return;
            }
        }
    } else {
        doze_fetch::lock::Lock::default()
    };
    for p in pins {
        let pin = doze_fetch::lock::EnginePin {
            resolved: p.resolved.clone(),
            source: "mirror".into(),
            hashes: p.hashes.clone(),
        };
        lock.record_engine(&p.engine, &p.spec, pin);
    }
    if let Err(e) = lock.save(path, None) {
        eprintln!("doze daemon: {} not updated: {e}", path.display());
    }
}
