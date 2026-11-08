#![no_main]
//! Any bytes, read as one record batch and as a Produce partition and as a run of batches, without a panic. A batch that
//! parses must survive stamping and still parse.

use bytes::Bytes;
use kafka_wire::batch::{Batch, Batches};
use kafka_wire::ids::{Offset, TimestampMs};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let bytes = Bytes::copy_from_slice(data);
    let _ = kafka_wire::batch::Peek::read(data);
    let _ = Batch::from_produce(
        &bytes,
        kafka_wire::batch::ProduceRules {
            version: 9,
            max_batch_bytes: 1 << 20,
        },
    );
    let _ = Batches::new(bytes.clone()).count();
    if let Ok(batch) = Batch::parse(bytes) {
        let _ = batch.record_times().map(Iterator::count);
        let rules = kafka_wire::batch::ScanRules {
            require_keys: true,
            log_append_time: false,
            now: TimestampMs::from_wire(1_000),
            before_max_ms: i64::MAX,
            after_max_ms: 1_000,
            max_inflated: 1 << 20,
        };
        let _ = batch.scan(&rules, &"t-0");
        let stamped = batch.stamped(Offset::ZERO, Some(TimestampMs::from_wire(1)));
        assert!(Batch::parse(stamped.to_bytes()).is_ok());
    }
});
