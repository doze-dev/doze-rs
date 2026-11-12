#![no_main]
//! Any text, read as a duration, without a panic or an overflow; a duration that parses parses the same with spaces
//! around it.

#[path = "../../src/duration.rs"]
mod duration;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Some(ms) = duration::millis(&text) {
        assert_eq!(duration::millis(&format!(" {text} ")), Some(ms));
    }
});
