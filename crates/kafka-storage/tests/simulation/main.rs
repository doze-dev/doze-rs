//! The partition log under random workloads and a disk that misbehaves.
//!
//! Each workload mixes appends, syncs, restarts (the process killed and started again), DeleteRecords, retention
//! and reads. The property is the one a Kafka user relies on, and the one the Go suite called
//! `TestPowerCutKeepsEveryAcksAllRecord`: after the power goes at any point, every record a flush
//! had acknowledged is there, in order, byte for byte, and whatever else survives is a prefix of what was tried.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod model;
mod reads;
mod rng;
mod sweeps;
mod world;
