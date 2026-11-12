#![no_main]
//! Any text as a timestamp: no panic, and an answer that parses is one we can print again.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Some(secs) = aws_core::time::parse_rfc3339(text) {
        let _ = aws_core::time::format_iso8601(secs);
    }
});
