#![no_main]
//! Any `lsof -Fpcn` or `ss -ltnpH` output reads without a panic, each listener under the port it holds.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    for held in [doze_desk::act::parse_lsof_all(&text), doze_desk::act::parse_ss_all(&text)] {
        for (port, h) in held {
            assert!(port > 0 && h.port == port);
        }
    }
});
