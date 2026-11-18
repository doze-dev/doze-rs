#![no_main]
//! Any text, read as a seed file, without a panic; every document it gives writes as BSON that reads back the same.

#[path = "../../src/bson.rs"]
#[allow(dead_code)]
mod bson;
#[path = "../../src/json.rs"]
#[allow(dead_code)]
mod json;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    if let Ok(docs) = json::documents(s) {
        for d in docs {
            let again = bson::Doc::from_bytes(&d.to_bytes()).expect("a seed document reads back");
            assert_eq!(format!("{d:?}"), format!("{again:?}"));
        }
    }
});
