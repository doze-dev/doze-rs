#![no_main]
//! Any text as a composite alarm's `AlarmRule`: no panic, no hang, whatever the shape.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = aws_cloudwatch::check_alarm_rule(text);
    }
});
