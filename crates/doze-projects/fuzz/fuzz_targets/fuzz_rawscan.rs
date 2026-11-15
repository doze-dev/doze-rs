#![no_main]
//! Any bytes as a record (project.json), without a panic: a record that decodes encodes, and what it encodes decodes
//! again to the same bytes (a newer doze's fields kept byte for byte, invalid UTF-8 and lone surrogates included).

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(p) = doze_projects::decode_bytes(data) else {
        return;
    };
    let Ok(once) = doze_projects::encode(&p) else {
        return;
    };
    let again = doze_projects::decode_bytes(&once).expect("what was written reads back");
    assert_eq!(doze_projects::encode(&again).ok(), Some(once));
});
