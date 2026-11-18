#![no_main]
//! Any text as a JSONPath, selected from a small document: no panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(path) = aws_asl::path::Path::parse(text) else { return };
    let doc = serde_json::json!({"a": [{"x": 1, "y": "p"}, {"x": 5}], "b": {"c": {"x": 2}}});
    let none = serde_json::json!({});
    let _ = path.select(aws_asl::path::Scope { data: &doc, context: &none, vars: &none });
});
