#![no_main]
//! Any text read for a log level without a panic, however long and whatever it lowers to; the rank agrees with it.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let line = String::from_utf8_lossy(data);
    let level = doze_display::level(&line);
    let (rank, tag) = doze_display::rank(&line);
    assert_eq!(level.unwrap_or(doze_display::LogLevel::Info), rank);
    assert_eq!(tag.is_some(), level.is_some());
});
