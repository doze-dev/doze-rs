#![no_main]
//! Any text as base64: no panic, and what decodes encodes to something that decodes to the same bytes.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Some(bytes) = aws_core::base64::decode(text) {
        assert_eq!(aws_core::base64::decode(&aws_core::base64::encode(&bytes)), Some(bytes));
    }
});
