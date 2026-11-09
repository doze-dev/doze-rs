//! Compaction under random keyed workloads and a disk that stops. The property a user of a compacted topic relies on:
//! whatever happens, including a stop between any two steps of a round, the latest record of every key is still in the log
//! at its own offset, nothing is in the log that was not written, offsets do not move, and the log can be read from
//! anywhere; and a round run again after a stop leaves the log as an uninterrupted one would have.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod sweeps;
mod world;
