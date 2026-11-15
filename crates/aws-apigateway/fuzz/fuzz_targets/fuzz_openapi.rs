#![no_main]
//! Any text as an OpenAPI document (JSON or the YAML subset): no panic, whatever the nesting.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        aws_apigateway::fuzz::openapi(text);
    }
});
