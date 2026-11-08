#![no_main]
//! Any frame payload, read as a request under every profile and feature set, without a panic.

use bytes::Bytes;
use kafka_wire::incoming;
use kafka_wire::profile::{Features, KafkaMajor, Profile};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let frame = Bytes::copy_from_slice(data);
    for major in KafkaMajor::RANGE {
        let Some(major) = KafkaMajor::new(major) else { continue };
        for (sasl, telemetry) in [(false, false), (true, true)] {
            let _ = incoming::read(frame.clone(), Profile::new(major), Features { sasl, telemetry });
        }
    }
});
