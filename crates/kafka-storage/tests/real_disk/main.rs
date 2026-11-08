//! The log on the real file system, and with real threads.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_storage::Trust;
use kafka_wire::batch::Batches;
use kafka_wire::ids::Offset;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use support::{Scratch, append, bases, batch, open};

fn offset(n: i64) -> Offset {
    Offset::new(n).unwrap()
}

#[test]
fn a_log_survives_being_closed_and_opened_and_serves_what_it_was_given() {
    let scratch = Scratch::new();
    let mut log = open(&scratch.partition(), 16 << 10, Trust::Unvouched);
    for i in 0..300 {
        append(&mut log, &[batch(100 + i)]);
    }
    log.sync().unwrap();
    let reader = log.reader();
    let before: Vec<_> = (0..300).collect();
    let mut seen = Vec::new();
    let mut from = reader.start_offset();
    while from < reader.end_offset() {
        let got = reader.read(from, usize::MAX, None).unwrap();
        for b in Batches::new(got) {
            let b = b.unwrap();
            seen.push(b.base_offset().unwrap().get());
            from = b
                .base_offset()
                .unwrap()
                .checked_add(b.record_count())
                .unwrap();
        }
    }
    assert_eq!(seen, before);
    log.close().unwrap();

    let reopened = open(&scratch.partition(), 16 << 10, Trust::Unvouched);
    assert_eq!(
        (reopened.start_offset(), reopened.end_offset()),
        (offset(0), offset(300))
    );
    let got = reopened
        .reader()
        .read(offset(299), usize::MAX, None)
        .unwrap();
    assert_eq!(bases(&got), vec![299]);
    let files = std::fs::read_dir(scratch.partition()).unwrap().count();
    assert!(
        files > 4,
        "the log rolled over several segments: {files} files"
    );
}

/// Readers on other threads see only whole payloads, in order, while the writer appends and rolls.
#[test]
fn readers_never_see_a_partial_append_while_the_writer_writes_and_rolls() {
    let scratch = Scratch::new();
    let mut log = open(&scratch.partition(), 16 << 10, Trust::Unvouched);
    let done = Arc::new(AtomicBool::new(false));
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let (reader, done) = (log.reader(), Arc::clone(&done));
            std::thread::spawn(move || {
                let mut next = offset(0);
                let mut batches = 0u64;
                loop {
                    let finished = done.load(Ordering::Acquire);
                    let got = reader.read(next, 8 << 10, None).unwrap();
                    for b in Batches::new(got) {
                        // Every batch is whole and valid (its checksum is checked by the iterator), and is the next.
                        let b = b.expect("a reader was handed a partial or damaged batch");
                        assert_eq!(b.base_offset(), Some(next));
                        next = next.checked_add(b.record_count()).unwrap();
                        batches += 1;
                    }
                    if finished && next == reader.end_offset() {
                        return batches;
                    }
                }
            })
        })
        .collect();
    for i in 0..3000usize {
        // Payloads of one to four batches of different sizes; a payload is visible whole or not at all.
        let payload: Vec<_> = (0..1 + i % 4)
            .map(|k| batch(40 + (i * 37 + k * 11) % 600))
            .collect();
        append(&mut log, &payload);
        if i % 50 == 0 {
            log.sync().unwrap();
        }
    }
    done.store(true, Ordering::Release);
    let total = log.end_offset().get() as u64;
    for r in readers {
        assert_eq!(
            r.join().unwrap(),
            total,
            "each reader saw every batch exactly once, in order"
        );
    }
}
