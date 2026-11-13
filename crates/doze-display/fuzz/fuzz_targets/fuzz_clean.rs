#![no_main]
//! Any bytes, cleaned, without a panic: what comes out is one line with no control character or escape left, and
//! cleaning it again changes nothing.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let once = doze_display::clean_bytes(data);
    assert!(!once.chars().any(|c| c.is_control()), "{once:?}");
    assert_eq!(doze_display::clean(&once), once);
    if let Ok(text) = std::str::from_utf8(data) {
        let lines = doze_display::clean_lines(text);
        assert_eq!(lines.split('\n').count(), text.split('\n').count());
    }
});
