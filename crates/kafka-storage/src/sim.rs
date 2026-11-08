//! A deterministic in-memory disk for tests (feature `sim`).
//!
//! It implements [`Fs`](crate::fs::Fs) and adds what a real disk will not do on request: fail the Nth operation of a kind,
//! fill up, stop the process after any operation, drop the pages of a file whose flush failed, and rebuild
//! what the disk holds if the power goes at that moment ([`SimFs::power_cut`]). Everything replays from a seed.
//!
//! Paths are absolute (`/data/orders-0/000.log`); the root exists.

mod control;
mod cut;
mod file;
mod ops;
mod state;

use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub use control::{Failure, Op};
pub use cut::{Cut, DataCut, NameCut};
pub use file::SimFile;

use control::{Control, Refused};
use state::{State, parent};

#[derive(Debug, Default)]
pub(crate) struct Inner {
    state: State,
    control: Control,
}

impl Inner {
    /// Runs the gate every operation passes: halted, failed on purpose, or fine.
    fn gate(&mut self, op: Op, path: &Path) -> Result<(), Refused> {
        self.control.begin(op, path)
    }
}

/// A simulated disk. Clones share it.
#[derive(Debug, Clone, Default)]
pub struct SimFs {
    inner: Arc<Mutex<Inner>>,
}

fn io_err(r: Refused) -> io::Error {
    r.error
}

fn absolute(path: &Path) -> io::Result<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::RootDir => out.push("/"),
            Component::Normal(name) if out.has_root() => out.push(name),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "simulated paths are absolute",
                ));
            }
        }
    }
    Ok(out)
}

impl SimFs {
    /// An empty disk with a root directory.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the time files are stamped with.
    pub fn set_now_ms(&self, now_ms: i64) {
        self.lock().control.now_ms = now_ms;
    }

    /// Fails the `nth` operation of kind `op` from now on, once.
    pub fn fail(&self, op: Op, nth: u64, failure: Failure) {
        self.lock().control.inject(Some(op), nth, failure);
    }

    /// Fails the `nth` counted operation of any kind from now on, once.
    pub fn fail_any(&self, nth: u64, failure: Failure) {
        self.lock().control.inject(None, nth, failure);
    }

    /// Stops the process after `ops` more counted operations: every later operation fails, as if the process
    /// were gone. Pair with [`SimFs::power_cut`] to see what the disk then holds.
    pub fn halt_after(&self, ops: u64) {
        self.lock().control.halt_after(ops);
    }

    /// Makes the disk hold at most `bytes` in all; a write past that is cut short and fails with
    /// `StorageFull`.
    pub fn set_capacity(&self, bytes: Option<u64>) {
        self.lock().control.capacity = bytes;
    }

    /// Whether a failed flush leaves the file's pages unflushable (the kernel's worst case); off by default.
    pub fn lose_pages_on_failed_fsync(&self, on: bool) {
        self.lock().control.lose_on_failed_fsync = on;
    }

    /// Forgets injected failures, a halt and a capacity.
    pub fn clear_faults(&self) {
        self.lock().control.clear();
    }

    /// How many counted operations have run.
    #[must_use]
    pub fn ops(&self) -> u64 {
        self.lock().control.counted()
    }

    /// How many operations of kind `op` have run (reads included).
    #[must_use]
    pub fn count(&self, op: Op) -> u64 {
        self.lock().control.count(op)
    }

    /// Every counted operation so far, in order.
    #[must_use]
    pub fn trace(&self) -> Vec<(Op, PathBuf)> {
        self.lock().control.trace()
    }

    /// The disk after the power goes. The old handles keep the old state; use the returned disk.
    #[must_use]
    pub fn power_cut(&self, cut: Cut) -> SimFs {
        let now_ms = self.lock().control.now_ms;
        let state = self.lock().state.cut(cut);
        let mut control = Control::default();
        control.now_ms = now_ms;
        SimFs {
            inner: Arc::new(Mutex::new(Inner { state, control })),
        }
    }

    /// The disk after the process is killed (`kill -9`) and started again. The kernel survives, so the page
    /// cache does: every byte and name the process had written is still there, and still not known to be on
    /// disk. A power cut after the restart takes unflushed data and names exactly as it would have before it.
    #[must_use]
    pub fn process_restart(&self) -> SimFs {
        let (state, now_ms) = {
            let g = self.lock();
            (g.state.clone(), g.control.now_ms)
        };
        let mut control = Control::default();
        control.now_ms = now_ms;
        SimFs {
            inner: Arc::new(Mutex::new(Inner { state, control })),
        }
    }

    /// The bytes of a file as the process sees them now, if there is one.
    #[must_use]
    pub fn contents(&self, path: &Path) -> Option<Vec<u8>> {
        let g = self.lock();
        let id = g.state.volatile.files.get(path)?;
        g.state.inodes.get(*id).map(|i| i.data.clone())
    }

    /// The bytes of a file a power cut would certainly keep: as of its last flush.
    #[must_use]
    pub fn durable_contents(&self, path: &Path) -> Option<Vec<u8>> {
        let g = self.lock();
        let id = g.state.volatile.files.get(path)?;
        g.state.inodes.get(*id).map(|i| i.durable.clone())
    }

    /// The names in a directory as the process sees them now.
    #[must_use]
    pub fn names(&self, dir: &Path) -> Vec<String> {
        let g = self.lock();
        let ns = &g.state.volatile;
        let files = ns.files.keys().filter(|p| parent(p) == dir);
        let dirs = ns
            .dirs
            .iter()
            .filter(|p| parent(p) == dir && p.as_path() != dir);
        let mut out: Vec<String> = files.chain(dirs).filter_map(|p| name_of(p)).collect();
        out.sort();
        out
    }
}

fn name_of(path: &Path) -> Option<String> {
    path.file_name()?.to_str().map(str::to_owned)
}

#[cfg(test)]
mod tests;
