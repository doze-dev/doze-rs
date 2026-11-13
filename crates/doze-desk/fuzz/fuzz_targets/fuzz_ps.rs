#![no_main]
//! Any `ps` output reads without a panic: every process found has its pid, and a daemon's config is a word of its line.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    for p in doze_desk::settings::leftovers::parse_ps(&text) {
        assert!(!p.exe.is_empty(), "a process with no program");
        assert!(p.daemon.is_empty() || text.contains(p.daemon.as_str()));
    }
});
