#![no_main]
//! Any text as a resource policy: no panic, whatever the shape.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = aws_secrets::check_resource_policy(text);
    }
});
