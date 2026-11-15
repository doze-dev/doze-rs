#![no_main]
//! Any answer from GitHub reads without a panic: a release, or the words About shows.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (status, body) = match data.split_first() {
        Some((s, rest)) => (if *s % 2 == 0 { 200 } else { u16::from(*s) * 3 }, String::from_utf8_lossy(rest)),
        None => return,
    };
    match doze_desk::settings::update::parse_release(status, &body) {
        Ok(r) => assert!(!r.tag.is_empty()),
        Err(e) => assert!(e.starts_with("GitHub answered ")),
    }
    let _ = doze_desk::settings::update::older(&body, "0.2.0");
});
