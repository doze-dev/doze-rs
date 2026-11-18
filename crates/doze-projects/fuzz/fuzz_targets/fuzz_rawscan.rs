#![no_main]
//! Any bytes as a record (project.json), without a panic: a record that decodes encodes, what it encodes reads back,
//! and from the second write on the bytes no longer change (a newer doze's fields kept byte for byte, invalid UTF-8
//! and lone surrogates included).
//!
//! Not from the first write: a key that is a known one in another case (`"Name"`) sets the field as Go's decoder does
//! and is also kept aside, as Go keeps it. When the exact key emptied the field in the same file, the first write
//! leaves the empty field out and keeps `"Name"`, so the next read takes `"Name"`'s value; Go's own record does the
//! same. The second write has the field again, and the exact key, sorted after its case variants, wins every read.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(p) = doze_projects::decode_bytes(data) else {
        return;
    };
    let Ok(once) = doze_projects::encode(&p) else {
        return;
    };
    let again = doze_projects::decode_bytes(&once).expect("what was written reads back");
    let Ok(twice) = doze_projects::encode(&again) else {
        panic!("what was written once writes again");
    };
    let third = doze_projects::decode_bytes(&twice).expect("what was written twice reads back");
    assert_eq!(doze_projects::encode(&third).ok(), Some(twice));
});
