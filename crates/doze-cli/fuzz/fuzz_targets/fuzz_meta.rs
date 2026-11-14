#![no_main]
//! Any text, as a module's documentation from the registry, without a panic.

use doze_cli::registry::meta;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = meta::parse(text);
    }
});
