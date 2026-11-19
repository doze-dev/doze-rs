#![no_main]
//! Any text, read as a doze-binaries mirror manifest, without a panic; an error's position is inside the text.

use doze_fetch::mirror;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Err(e) = mirror::parse_manifest(text) {
        assert!(e.at.is_none_or(|at| at <= text.len()));
    }
});
