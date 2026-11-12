#![no_main]
//! Any client-final message: never a panic. What the parser accepts ends in `p=` and holds a printable nonce,
//! and a message the machine accepts as a login would need a proof only the password can make: fuzzing finds
//! none, so a login here is a failure.

use std::num::NonZeroU32;
use std::sync::Arc;

use kafka_auth::scram::keys::Verifier;
use kafka_auth::scram::{self, Kind, parse};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(m) = parse::last(data) {
        assert!(data.is_ascii() && !data.contains(&0));
        assert!(!m.nonce.is_empty() && !m.nonce.contains(','));
        assert!(data.windows(3).any(|w| w == b",p="));
    }
    let Some(iterations) = NonZeroU32::new(4096) else {
        return;
    };
    let verifier = Arc::new(Verifier::derive(Kind::Sha256, b"pw", b"salt", iterations));
    let Ok((_, awaiting)) = scram::first(
        Kind::Sha256,
        b"n,,n=alice,r=client",
        "server",
        |_| Some(verifier.clone()),
    ) else {
        return;
    };
    // The nonce the machine expects is "clientserver": a message that has it still needs the right proof.
    if awaiting.finish(data).is_ok() {
        panic!("a login without the password");
    }
});
