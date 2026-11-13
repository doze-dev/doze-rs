#![no_main]
//! Any text as a schedule expression: no panic, and a schedule that parses can be asked for its next time.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data)
        && let Ok(schedule) = aws_cron::parse(text)
    {
        let _ = schedule.next_after(0, 1_700_000_000_000);
    }
});
