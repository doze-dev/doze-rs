#![no_main]
//! Any text, as a duration on the command line, without a panic; what parses is positive and under a year.

use doze_cli::cli::values::duration;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Ok(d) = duration(text) {
        assert!(!d.is_zero() && d.as_secs() < 365 * 86_400);
    }
});
