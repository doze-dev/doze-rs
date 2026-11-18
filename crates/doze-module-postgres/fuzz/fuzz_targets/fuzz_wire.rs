#![no_main]
//! Any bytes, read as a client's first packet and a startup body, and as server messages, without a panic; keys read
//! back as written.

#[path = "../../src/wire.rs"]
#[allow(dead_code)]
mod wire;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some((header, body)) = data.split_first_chunk::<8>() {
        if let Ok(first) = wire::first(*header) {
            match first {
                wire::First::Startup(_) => {
                    let _ = wire::startup_database(*header, body);
                }
                wire::First::Cancel(_) => {
                    if let Some(k) = wire::cancel_key(body) {
                        let again = wire::cancel_request(&k);
                        assert_eq!(wire::cancel_key(again.get(8..).unwrap_or_default()), Some(k));
                    }
                }
                wire::First::Encrypt(_) => {}
            }
        }
    }
    if let Some((header, payload)) = data.split_first_chunk::<5>()
        && let Ok((kind, _)) = wire::message(*header)
        && let Some(k) = wire::backend_key(kind, payload)
    {
        assert_eq!(wire::key_payload(&k), payload);
    }
    let _ = wire::error_response("08P01", &String::from_utf8_lossy(data));
});
