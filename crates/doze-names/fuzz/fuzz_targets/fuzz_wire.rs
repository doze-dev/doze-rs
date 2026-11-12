#![no_main]
//! Any bytes, as a query to the resolver and as an upstream answer to relay, without a panic: an answer echoes the
//! query's id and question; a relayed message keeps its length.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let resolve = |h: &str| h.ends_with(".doze").then(|| std::net::Ipv4Addr::new(127, 0, 0, 9));
    if let Some((resp, _)) = doze_names::fuzzing::answer(data, &resolve) {
        assert_eq!(resp.get(..2), data.get(..2));
    }
    assert_eq!(doze_names::fuzzing::cap_ttl(data, 10).len(), data.len());
});
