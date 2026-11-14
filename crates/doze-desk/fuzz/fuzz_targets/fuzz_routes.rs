#![no_main]
//! Any routes record reads without a panic, and only valid domains come out of it: they reach a privileged script.

use libfuzzer_sys::fuzz_target;
use doze_desk::settings::leftovers::{parse_routes, route_script, valid_domain};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let m = parse_routes(&text);
    for d in m.keys() {
        assert!(valid_domain(d), "{d}");
        assert!(d.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.'));
    }
    let script = route_script(&m.keys().cloned().collect::<Vec<_>>());
    assert!(!script.contains(';') && !script.contains('`') && !script.contains('$'));
});
