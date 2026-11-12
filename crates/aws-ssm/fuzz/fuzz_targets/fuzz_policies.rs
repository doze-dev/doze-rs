#![no_main]
//! Any text as a parameter-policies document: no panic, whatever the shape.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = aws_ssm::check_policies(text);
    }
});
