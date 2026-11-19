#![no_main]
//! Any text as a PEM certificate, private key and chain: no panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        aws_iam::fuzz::certificate(text);
    }
});
