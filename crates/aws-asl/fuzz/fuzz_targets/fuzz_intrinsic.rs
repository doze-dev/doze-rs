#![no_main]
//! Any text as an intrinsic call, run against a small document: no panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let doc = serde_json::json!({"a": "x", "n": 3, "list": [1, 2, 3], "obj": {"k": 1}});
    let none = serde_json::json!({});
    let scope = aws_asl::path::Scope { data: &doc, context: &none, vars: &none };
    let _ = aws_asl::intrinsic::eval(text, scope, &aws_asl::Env::new(0, 1));
});
