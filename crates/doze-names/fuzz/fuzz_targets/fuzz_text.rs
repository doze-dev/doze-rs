#![no_main]
//! Any text, as `scutil --dns` output and as a resolv.conf, without a panic; every server found is on port 53.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    for s in doze_names::fuzzing::scutil_servers(&text).iter().chain(&doze_names::fuzzing::resolv_conf_servers(&text)) {
        assert_eq!(s.port(), 53);
    }
});
