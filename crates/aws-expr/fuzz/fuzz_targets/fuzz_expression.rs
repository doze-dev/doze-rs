#![no_main]
//! Any text as each kind of expression, as an attribute value in JSON, and as a number: no panic, whatever the shape.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = aws_expr::parse_all(text);
    }
});
