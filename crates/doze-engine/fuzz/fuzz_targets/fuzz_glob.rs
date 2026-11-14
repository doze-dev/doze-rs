#![no_main]
//! Any pattern against any name: an answer, never a panic, and quickly (the fuzzer's timeout catches a matcher that
//! backtracks without bound). A pattern with no wildcard matches exactly itself.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (pattern, name) = text.split_once('\n').unwrap_or((&text, ""));
    let got = doze_engine::tasks::inputs::matches(pattern, name);
    if !pattern.contains(['*', '?', '[', '\\']) {
        assert_eq!(got, pattern == name);
    }
});
