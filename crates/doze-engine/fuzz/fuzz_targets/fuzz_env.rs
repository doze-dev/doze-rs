#![no_main]
//! Any text, read as an env file, without a panic; every key it yields is trimmed and appears in the text.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    for (k, _) in doze_engine::process::parse_env(&text) {
        assert_eq!(k.trim(), k);
        assert!(text.contains(k.as_str()));
    }
});
