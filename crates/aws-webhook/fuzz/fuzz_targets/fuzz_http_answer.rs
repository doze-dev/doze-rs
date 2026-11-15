#![no_main]
//! Any bytes as an HTTP answer (head, chunked framing): no panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = aws_webhook::fuzz::http_answer(data);
});
