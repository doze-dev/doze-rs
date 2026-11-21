#![no_main]
//! Any User-Agent: never a panic, and the label kept is at most `MOST` bytes of printable ASCII and spaces.

use aws_console::agent::{MOST, label};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let got = label(text);
    assert!(got.len() <= MOST, "{text:?}");
    assert!(got.bytes().all(|b| b == b' ' || b.is_ascii_graphic()), "{text:?} gave {got:?}");
});
