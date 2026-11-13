#![no_main]
//! Any text, read as the engine's `INFO` and as `MONITOR` lines, without a panic; a parsed monitor line's parts are
//! all taken from the line.

use doze_resp::{fields, parse_info, parse_monitor};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    for value in parse_info(&text).values() {
        let _ = fields(value);
    }
    for line in text.lines() {
        if let Some(m) = parse_monitor(line) {
            assert!(line.starts_with(&m.time));
            assert!(line.contains(&m.client) && line.ends_with(&m.command));
        }
    }
});
