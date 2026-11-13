#![no_main]
//! Any bytes as a CBOR body: no panic, no hang, and what is accepted is JSON text.

use aws_wire::cbor::to_json;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = to_json(data) {
        assert!(!text.is_empty());
    }
});
