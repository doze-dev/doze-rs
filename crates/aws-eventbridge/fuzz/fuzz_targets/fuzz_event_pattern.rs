#![no_main]
//! Any text as an event pattern: no panic, whatever the shape or the nesting.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = aws_eventbridge::check_event_pattern(text);
    }
});
