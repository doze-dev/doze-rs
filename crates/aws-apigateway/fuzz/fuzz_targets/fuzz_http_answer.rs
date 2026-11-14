#![no_main]
//! Any bytes as an HTTP integration's answer (head, chunked framing): no panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    aws_apigateway::fuzz::http_answer(data);
});
