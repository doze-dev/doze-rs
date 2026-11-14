#![no_main]
//! Any text as a PartiQL statement: read or refused, never a panic, whatever the nesting or the length.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = aws_dynamodb::check_partiql(text);
    }
});
