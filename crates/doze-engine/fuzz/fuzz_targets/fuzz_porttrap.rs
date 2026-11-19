#![no_main]
//! Any system listing (`ps`, `lsof -F`, `ss`): an answer, never a panic; the tree always starts at its root, and no port
//! is listed twice or as 0.

use libfuzzer_sys::fuzz_target;
use doze_engine::porttrap;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let tree = porttrap::descendants(7, &text);
    assert_eq!(tree.first(), Some(&7));
    for ports in [porttrap::lsof_ports(&text), porttrap::ss_ports(&text, &tree)] {
        let mut seen = std::collections::BTreeSet::new();
        assert!(ports.iter().all(|p| *p > 0 && seen.insert(*p)));
    }
    if let Some(h) = porttrap::parse_holder(&text) {
        assert!(h.pid > 0);
    }
});
