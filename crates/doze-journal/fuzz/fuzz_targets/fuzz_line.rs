#![no_main]
//! Any bytes, as one journal line and as a whole journal file, without a panic: whatever reads back has a sequence
//! number, and a line reads the same alone as in a file.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let one = doze_journal::parse_line(data);
    if let Some(e) = &one {
        assert_ne!(e.seq, 0);
    }
    if let Ok(events) = doze_journal::parse(data) {
        assert!(events.iter().all(|e| e.seq != 0));
        if !data.contains(&b'\n') && !data.ends_with(b"\r") {
            assert_eq!(events, one.into_iter().collect::<Vec<_>>());
        }
    }
});
