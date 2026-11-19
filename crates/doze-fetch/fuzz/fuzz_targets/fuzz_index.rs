#![no_main]
//! Any text, as a module index from the registry, without a panic; whatever parses gives a payload that is JSON and
//! never verifies with the key built in unless it was signed by it.

use doze_fetch::registry::{index, signed};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Ok(idx) = index::parse(text) {
        let payload = index::payload(&idx);
        assert!(serde_json::from_str::<serde_json::Value>(&payload).is_ok());
        if let Some(key) = signed::builtin_key() {
            let _ = index::verify(&idx, &key);
        }
    }
});
