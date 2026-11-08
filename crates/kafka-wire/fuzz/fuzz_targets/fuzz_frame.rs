#![no_main]
//! Any byte stream, in any authentication state, split into frames without a panic.

use bytes::BytesMut;
use kafka_wire::frame::{FrameDecoder, Trust};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for trust in [Trust::Anonymous, Trust::Authenticated] {
        let mut src = BytesMut::from(data);
        let mut decoder = FrameDecoder::new(trust);
        while let Ok(Some(_)) = decoder.decode(&mut src) {}
    }
});
