#![no_main]
//! Any text as YAML: no panic, no hang, whatever the shape; and what is read writes back as YAML that reads.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data)
        && let Ok(value) = aws_yaml::parse(text)
    {
        assert!(aws_yaml::parse(&aws_yaml::emit(&value)).is_ok());
    }
});
