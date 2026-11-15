#![no_main]
//! Any crash text, `lsof` output or `ss` output reads without a panic: a port found is one a process could listen on,
//! and a holder found has a pid.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Some(p) = doze_desk::ports::port_in_use(text.lines()) {
        assert!(p >= 1, "a port doze could not listen on");
    }
    for h in [doze_desk::ports::parse_lsof(8000, &text), doze_desk::ports::parse_ss(8000, &text)]
        .into_iter()
        .flatten()
    {
        assert!(h.pid > 0 && h.port == 8000);
    }
});
