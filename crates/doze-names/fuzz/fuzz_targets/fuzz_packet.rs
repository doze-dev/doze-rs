#![no_main]
//! Any bytes, as a packet off the daemon's interface, without a panic; a packet written back is a whole IPv4 packet
//! no longer than the buffer.

use doze_names::nat::{PortTable, packet_len, translate};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut table = PortTable::default();
    table.add([198, 19, 3, 7], 5432, 51000);
    let mut p = data.to_vec();
    if translate(&mut p, [198, 19, 0, 1], &table) {
        assert!(packet_len(&p) <= p.len() && p.first().map(|b| b >> 4) == Some(4));
    }
});
