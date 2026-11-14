#![no_main]
//! Any text as a mapping template: read and run without a panic, a stack overflow or a runaway loop.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        aws_apigateway::fuzz::vtl(text);
    }
});
