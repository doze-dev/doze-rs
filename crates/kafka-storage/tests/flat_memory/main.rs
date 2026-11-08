//! The log is never pulled into the heap: what it costs in memory does not grow with the data it holds.
//!
//! One test, because the allocator counts the whole process.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "../real_disk/support.rs"]
mod support;

use std::alloc::System;

use kafka_storage::Trust;
use kafka_wire::ids::Offset;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use support::{Scratch, append, batch, open};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const SEGMENT: u64 = 4 << 20;
const BODY: usize = 4096;

/// A closed partition holding about `mib` MiB, in segments of 4 MiB.
fn partition_of(mib: usize) -> Scratch {
    let scratch = Scratch::new();
    let mut log = open(&scratch.partition(), SEGMENT, Trust::Unvouched);
    let payload: Vec<_> = (0..8).map(|_| batch(BODY)).collect();
    for _ in 0..(mib << 20) / (8 * (BODY + 61)) {
        append(&mut log, &payload);
    }
    log.close().unwrap();
    scratch
}

/// (bytes allocated in all, bytes still held) by `f`.
fn cost<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    let region = Region::new(GLOBAL);
    let out = f();
    let stats = region.change();
    (
        out,
        stats.bytes_allocated,
        stats
            .bytes_allocated
            .saturating_sub(stats.bytes_deallocated),
    )
}

#[test]
fn memory_does_not_follow_the_data() {
    let (small, large) = (partition_of(8), partition_of(64));

    // Opening reads the active segment's headers through one window and keeps nothing: it costs the same for
    // a log eight times the size, and holds almost nothing once open.
    let (log_small, gross_small, _) = cost(|| open(&small.partition(), SEGMENT, Trust::Unvouched));
    let (log_large, gross_large, held_large) =
        cost(|| open(&large.partition(), SEGMENT, Trust::Unvouched));
    assert!(
        gross_large < 3 << 20,
        "opening a 64 MiB log allocated {gross_large} bytes in all"
    );
    assert!(
        held_large < 64 << 10,
        "an open 64 MiB log holds {held_large} bytes"
    );
    assert!(
        gross_large <= gross_small + (256 << 10),
        "open cost grew with the log: {gross_small} for 8 MiB, {gross_large} for 64 MiB"
    );

    // A fetch costs about the bytes it returns: one buffer, no copies of it. (It asks the file for 16 KiB more
    // than it was asked to return, to step over the batches before the offset in the same read.)
    let reader = log_large.reader();
    let from = Offset::new(100).unwrap();
    let (got, gross, _) = cost(|| reader.read(from, 256 << 10, None).unwrap());
    assert!(
        got.len() >= 200 << 10,
        "the read returned {} bytes",
        got.len()
    );
    assert!(
        gross < got.len() + (20 << 10),
        "a {}-byte read allocated {gross} bytes",
        got.len()
    );
    drop(got);

    // Appending keeps nothing per append: the batches go to the file and the index is on disk.
    let mut log = log_large;
    let payload: Vec<_> = (0..8).map(|_| batch(BODY)).collect();
    let (_, _, held) = cost(|| {
        for _ in 0..500 {
            append(&mut log, &payload);
        }
    });
    assert!(
        held < 64 << 10,
        "500 appends of 32 KiB left {held} bytes held"
    );
    drop((log, log_small));
}
