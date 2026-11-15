#![no_main]
//! Any `doze --json doctor` output reads without a panic, and every check kept is the machine's.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Some(d) = doze_desk::library::parse_doctor(&text) {
        for c in &d.checks {
            assert!(c.section == "names" || c.section == "domains");
        }
    }
});
