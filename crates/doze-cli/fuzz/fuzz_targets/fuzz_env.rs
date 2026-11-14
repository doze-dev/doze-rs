#![no_main]
//! Any value of TZ, which doze reads to stamp log lines with this machine's wall clock: the zone falls back to the
//! system's or UTC, and a time is drawn in it, without a panic. (DOZE_JSON, DOZE_YES, NO_COLOR are compared, not parsed.)

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = doze_cli::clock::zone(Some(&text));
    let _ = doze_cli::clock::wall(&doze_control::Timestamp::parse(&text).unwrap_or_default(), &doze_cli::clock::zone(None));
});
