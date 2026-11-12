#![no_main]
//! Any text read as Go reads a float without a panic. A figure it takes is finite unless it was written as an
//! infinity or a NaN, and a plain decimal reads as Rust's own parser reads it.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let Some(v) = doze_display::parse_float(&s) else {
        return;
    };
    let lower = s.to_ascii_lowercase();
    if !lower.contains("inf") && !lower.contains("nan") {
        assert!(v.is_finite(), "{s:?} read as {v}");
    }
    if s.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        assert_eq!(s.parse::<f64>().ok(), Some(v));
    }
});
