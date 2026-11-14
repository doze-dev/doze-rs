#![no_main]
//! Any text, as the registry's release file, without a panic; nothing unsigned is ever accepted.

use doze_cli::registry::{latest, signed};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (Ok(text), Some(key)) = (std::str::from_utf8(data), signed::builtin_key()) else { return };
    assert!(latest::parse(text, &key).is_err() || text.contains("signature"));
});
