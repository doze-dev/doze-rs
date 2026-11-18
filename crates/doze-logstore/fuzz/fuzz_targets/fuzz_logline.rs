#![no_main]
//! Any bytes as a line of a log segment: parsed or skipped, never a panic; what parses keeps its text whole.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(l) = doze_logstore::parse_line("i", 1, data) {
        assert_eq!(l.stamp.len(), 27);
        assert!(String::from_utf8_lossy(data).ends_with(&l.text));
    }
});
