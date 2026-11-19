#![no_main]
//! Any text as a Logs Insights query, and any text as the one event it runs over: no panic, whatever the shape.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        // The first line is the query; the rest is the event's message.
        let (query, message) = text.split_once('\n').unwrap_or((text, ""));
        let _ = aws_logs::check_insights_query(query, message);
    }
});
