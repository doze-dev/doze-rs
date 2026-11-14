#![no_main]
//! Any text, read as a doze.hcl (blocks, attributes, references, the Grid's quotes), as the CLI's `--json` envelopes
//! (`doze status`, `doze doctor`) and as the loader's error words, without a panic; every reference read names a block
//! the text could have declared, and every quote is a line of the text, closed up.

use doze_desk::library::parse_doctor;
use doze_desk::overview::{config_error, parse_status, scan};
use libfuzzer_sys::fuzz_target;
use std::path::Path;
use std::time::UNIX_EPOCH;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let f = scan(text);
    for b in &f.blocks {
        for n in &b.needs {
            assert!(text.contains(n.as_str()), "{n} is not in the text");
        }
    }
    let _ = parse_status(text);
    let _ = parse_doctor(text);
    let ce = config_error(Path::new("/nonexistent/doze.hcl"), text, None, UNIX_EPOCH);
    assert!(ce.excerpt.is_empty());
});
