#![no_main]
//! Any bytes as a bucket deployment's asset archive: no panic, bounded memory, whatever the shape.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = aws_cloudformation::check_zip(data);
});
