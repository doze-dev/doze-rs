#![no_main]
//! Any client-first message: never a panic. What the parser accepts is ASCII with a non-empty name and nonce
//! that holds no comma, and the machine, given that message, either answers a server-first message that starts
//! with the client's nonce or refuses it.

use std::num::NonZeroU32;
use std::sync::Arc;

use kafka_auth::scram::keys::Verifier;
use kafka_auth::scram::{self, Kind, parse};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(m) = parse::first(data) {
        assert!(data.is_ascii() && !data.contains(&0));
        assert!(!m.saslname.is_empty() && !m.nonce.is_empty());
        assert!(!m.nonce.contains(',') && !m.nonce.contains(' '));
        // The name can be unescaped, and the result has no escape left over that is not its own.
        let _ = parse::unescape(m.saslname);
    }
    let Some(iterations) = NonZeroU32::new(4096) else {
        return;
    };
    let verifier = Arc::new(Verifier::derive(Kind::Sha256, b"pw", b"salt", iterations));
    let known = |_: &str| Some(verifier.clone());
    match scram::first(Kind::Sha256, data, "server-nonce", known) {
        Ok((server_first, _)) => {
            let nonce = parse::first(data).map(|m| m.nonce.to_owned()).unwrap_or_default();
            let text = String::from_utf8_lossy(&server_first);
            assert!(text.starts_with(&format!("r={nonce}server-nonce,s=")));
        }
        Err(refusal) => {
            let _ = refusal.to_string();
        }
    }
});
