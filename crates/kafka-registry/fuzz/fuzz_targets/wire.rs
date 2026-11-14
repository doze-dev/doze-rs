#![no_main]
//! Any record value: never a panic; what the header names is what was framed, and message indexes that are read write
//! back to bytes that read as the same indexes.

use kafka_registry::wire::{header, message_indexes, put_message_indexes, split};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((id, rest)) = split(data) {
        let mut again = header(id).to_vec();
        again.extend_from_slice(rest);
        assert_eq!(again, data);
    }
    if let Ok((idx, rest)) = message_indexes(data) {
        let mut out = Vec::new();
        put_message_indexes(&mut out, &idx);
        out.extend_from_slice(rest);
        let (back, tail) = message_indexes(&out).expect("written indexes read again");
        assert_eq!((back, tail), (idx, rest));
    }
});
