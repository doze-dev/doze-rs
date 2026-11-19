#![no_main]
//! Any text as a transformer configuration (JSON) and any text as the event it runs over: no panic, whatever the shape.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        // The first line is the configuration; the rest is the event's message.
        let (config, message) = text.split_once('\n').unwrap_or((text, ""));
        let _ = aws_logs::check_transformer(config, message);
    }
});
