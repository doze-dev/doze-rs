#![no_main]
//! Any text as a filter policy, for either scope: no panic, whatever the shape.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some((scope, rest)) = data.split_first()
        && let Ok(text) = std::str::from_utf8(rest)
    {
        let _ = aws_sns::check_filter_policy(text, scope & 1 == 1);
    }
});
