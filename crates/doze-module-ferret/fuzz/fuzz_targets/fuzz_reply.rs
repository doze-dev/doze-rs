#![no_main]
//! Any bytes, read as an OP_MSG reply to request 1, without a panic; then its outcome, also without one.

#[path = "../../src/bson.rs"]
#[allow(dead_code)]
mod bson;
#[path = "../../src/msg.rs"]
#[allow(dead_code)]
mod msg;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(body) = msg::reply(data, 1) {
        let _ = msg::outcome(body);
    }
});
