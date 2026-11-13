#![no_main]
//! Any text as a policy document of each kind: no panic, whatever the shape.

use aws_iam_policy::{Kind, check, parse};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = parse(text);
        for kind in [Kind::Identity, Kind::Trust, Kind::Resource] {
            let _ = check(text, kind);
        }
    }
});
