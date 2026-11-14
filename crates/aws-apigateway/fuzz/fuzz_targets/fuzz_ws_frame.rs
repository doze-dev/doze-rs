#![no_main]
//! Any bytes as WebSocket frames from a client (masking, lengths, fragments): no panic, and no buffer past the cap.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    aws_apigateway::fuzz::ws_frames(data);
});
