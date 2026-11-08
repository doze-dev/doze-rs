#![no_main]
//! Any bytes, read as one record batch and as a Produce partition and as a run of batches, without a panic. A batch that
//! parses must survive stamping and still parse.

use bytes::Bytes;
use kafka_wire::batch::{Batch, Batches};
use kafka_wire::ids::{Offset, TimestampMs};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let bytes = Bytes::copy_from_slice(data);
    let _ = Batch::from_produce(&bytes);
    let _ = Batches::new(bytes.clone()).count();
    if let Ok(batch) = Batch::parse(bytes) {
        let stamped = batch
            .with_base_offset(Offset::ZERO)
            .with_log_append_time(TimestampMs::from_wire(1));
        assert!(Batch::parse(stamped.into_bytes()).is_ok());
    }
});
