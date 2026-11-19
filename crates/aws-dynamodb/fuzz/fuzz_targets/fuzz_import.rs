#![no_main]
//! Any text as the object of an import (DynamoDB JSON lines, Ion, CSV): items read or errors counted, never a panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = aws_dynamodb::check_import(text);
    }
});
