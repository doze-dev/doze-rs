//! Counting operations, injecting failures, and stopping the process after a chosen one.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

/// An operation at the file seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    /// A file that was not there is created.
    Create,
    /// Bytes written to a file.
    Write,
    /// A file cut or extended to a length.
    Truncate,
    /// A file renamed.
    Rename,
    /// A file removed.
    Remove,
    /// A directory created.
    Mkdir,
    /// A file's contents flushed.
    Fsync,
    /// A directory's entries flushed.
    DirSync,
    /// Bytes read. Reads can fail but are not counted for [`SimFs::halt_after`](super::SimFs::halt_after).
    Read,
}

/// How an injected failure presents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// A device error (`EIO`).
    Io,
    /// No space left on the device (`ENOSPC`).
    Full,
}

impl Failure {
    pub(super) fn error(self) -> io::Error {
        match self {
            Self::Io => io::Error::other("simulated I/O error"),
            Self::Full => io::Error::from(io::ErrorKind::StorageFull),
        }
    }
}

/// A failure to inject once.
#[derive(Debug, Clone, Copy)]
struct Fault {
    /// The kind of operation, or `None` to count every counted operation.
    op: Option<Op>,
    /// Which one (1 is the first from now on).
    nth: u64,
    failure: Failure,
}

/// The reason an operation did not run.
#[derive(Debug)]
pub(super) struct Refused {
    pub error: io::Error,
    /// A failure injected on purpose, as opposed to the process having been stopped.
    pub injected: bool,
}

/// Operation counting and fault state of one simulated disk.
#[derive(Debug, Default)]
pub(super) struct Control {
    counted: u64,
    per_op: HashMap<Op, u64>,
    halt_after: Option<u64>,
    faults: Vec<Fault>,
    pub capacity: Option<u64>,
    /// After a failed fsync the dirty pages are dropped from the durability account: a later fsync of the
    /// same file succeeds and makes none of them durable (the kernel's behaviour that makes retrying wrong).
    pub lose_on_failed_fsync: bool,
    pub now_ms: i64,
    trace: Vec<(Op, PathBuf)>,
}

impl Control {
    /// Called before every operation: says whether it may run.
    pub(super) fn begin(&mut self, op: Op, path: &Path) -> Result<(), Refused> {
        if self.halt_after.is_some_and(|k| self.counted >= k) {
            return Err(Refused {
                error: io::Error::other("the process was stopped"),
                injected: false,
            });
        }
        if op != Op::Read {
            self.counted += 1;
            self.trace.push((op, path.to_path_buf()));
        }
        let seen = self.per_op.entry(op).or_insert(0);
        *seen += 1;
        let (seen, counted) = (*seen, self.counted);
        let hit = self.faults.iter().position(|f| match f.op {
            Some(o) => o == op && f.nth == seen,
            None => op != Op::Read && f.nth == counted,
        });
        match hit {
            Some(i) => Err(Refused {
                error: self.faults.remove(i).failure.error(),
                injected: true,
            }),
            None => Ok(()),
        }
    }

    pub(super) fn inject(&mut self, op: Option<Op>, nth: u64, failure: Failure) {
        // Numbering is from now on, whatever has run before.
        let seen = op.map_or(self.counted, |o| self.per_op.get(&o).copied().unwrap_or(0));
        self.faults.push(Fault {
            op,
            nth: seen + nth,
            failure,
        });
    }

    pub(super) fn halt_after(&mut self, ops: u64) {
        self.halt_after = Some(self.counted + ops);
    }

    pub(super) fn count(&self, op: Op) -> u64 {
        self.per_op.get(&op).copied().unwrap_or(0)
    }

    pub(super) fn counted(&self) -> u64 {
        self.counted
    }

    pub(super) fn clear(&mut self) {
        self.faults.clear();
        self.halt_after = None;
        self.capacity = None;
    }

    pub(super) fn trace(&self) -> Vec<(Op, PathBuf)> {
        self.trace.clone()
    }
}
