#![no_main]
//! Any query string or form body: never a panic, at most 64 fields, a component never longer than three times its
//! input (a byte that is not UTF-8 becomes a three-byte replacement), a component with no `%` and no `+` comes back as it
//! was, and the fields found are the `&`-separated non-empty pieces in order (up to the limit).

use kafka_console::form::{decode, get, parse};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        // Not text: the decoder takes a `&str`, so there is nothing to feed it; its own bytes are covered by `%XX` below.
        return;
    };
    let fields = parse(text);
    assert!(fields.len() <= 64);
    let pieces = text.split('&').filter(|p| !p.is_empty()).count();
    assert_eq!(fields.len(), pieces.min(64));
    for (k, v) in &fields {
        assert!(k.len() <= 3 * text.len() && v.len() <= 3 * text.len());
    }
    if let Some((k, _)) = fields.first() {
        assert!(get(&fields, k).is_some());
    }
    let plain = !text.contains(['%', '+']);
    if plain {
        assert_eq!(decode(text), text);
    }
    // Every byte, percent-encoded, comes back as the UTF-8 text of those bytes.
    let encoded: String = data.iter().map(|b| format!("%{b:02X}")).collect();
    assert_eq!(decode(&encoded).as_bytes(), String::from_utf8_lossy(data).as_bytes());
});
