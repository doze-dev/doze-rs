#![no_main]
//! Any text as an outbound URL: no panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = aws_webhook::fuzz::url(text);
    }
});
