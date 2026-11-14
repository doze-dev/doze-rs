#![no_main]
//! Any text: never a panic, and JSON that is read is written back as JSON that reads as the same value.

use kafka_registry::json::Json;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = kafka_registry::json::depth(text);
    if let Ok(v) = Json::parse(text) {
        let again = Json::parse(&v.to_text()).expect("written JSON reads again");
        assert_eq!(again, v);
    }
});
