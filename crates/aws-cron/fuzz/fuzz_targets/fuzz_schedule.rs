#![no_main]
//! Any text as a schedule expression (EventBridge's and Scheduler's): no panic, and a schedule that parses can be asked for its next
//! time, in UTC and in a zone with daylight saving time.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(schedule) = aws_cron::parse(text) {
        let _ = schedule.next_after(0, 1_700_000_000_000);
    }
    if let Ok(schedule) = aws_cron::parse_scheduler(text)
        && let Some(zone) = aws_cron::Zone::named("America/New_York")
    {
        let _ = schedule.next_in(&zone, 0, 1_700_000_000_000);
    }
});
