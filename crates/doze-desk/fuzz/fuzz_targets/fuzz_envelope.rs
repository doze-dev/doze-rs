#![no_main]
//! Any CLI output reads without a panic: a run that exited badly never reads as done, and a failure keeps at most
//! forty printed lines and says something.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (out, err) = text.split_at(text.len() / 2);
    let ran = doze_desk::act::read_ran(false, out, err);
    assert!(!ran.ok);
    let f = doze_desk::act::failure_of("Couldn't start x", std::path::Path::new("/nonexistent"), &ran.printed);
    assert!(f.lines.len() <= doze_desk::act::LINES_KEPT);
});
