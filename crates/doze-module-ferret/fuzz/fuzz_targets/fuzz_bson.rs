#![no_main]
//! Any bytes, read as a BSON document, without a panic; a document that reads writes back to bytes that read the same.

#[path = "../../src/bson.rs"]
#[allow(dead_code)]
mod bson;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = bson::Doc::from_bytes(data) {
        let again = bson::Doc::from_bytes(&doc.to_bytes()).expect("a document read once reads again");
        assert_eq!(format!("{doc:?}"), format!("{again:?}"));
    }
});
