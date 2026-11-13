#![no_main]
//! Any text as a JSON Web Token: no panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        aws_apigateway::fuzz::jwt(text);
    }
});
