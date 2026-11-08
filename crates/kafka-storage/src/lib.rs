//! Partition logs on disk: segments, index, recovery, retention, compaction.
//!
//! Everything here is synchronous and runtime-free: a log is a value with `&mut self` methods, and every
//! file operation goes through [`fs::Fs`], which is what lets the whole crate run on a simulated disk.

pub mod fs;
#[cfg(any(test, feature = "sim"))]
pub mod sim;
