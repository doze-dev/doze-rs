#![no_main]
//! Any text as a filter pattern, and any text as the message it is held against: no panic, whatever the shape.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        // The first line is the pattern; the rest is the message.
        let (pattern, message) = text.split_once('\n').unwrap_or((text, ""));
        let _ = aws_logs::check_filter_pattern(pattern, message);
    }
});
