#![no_main]
//! Any text, as a doze.lock (hand edits, merges), without a panic; an error's position is inside the text.

use doze_fetch::lock;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Err(e) = lock::parse(text) {
        assert!(e.at.is_none_or(|at| at <= text.len()));
    }
});
