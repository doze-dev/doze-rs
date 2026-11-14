//! Any text: parsing returns a body or a positioned diagnostic, never panics, never hangs.
#![no_main]

use doze_hcl::syntax::{Source, parse_body};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let src = Source::new(text);
        if let Err(ds) = parse_body(&src) {
            assert!(ds.has_errors());
            for d in ds.iter() {
                let at = d.subject.unwrap_or_default();
                assert!(at.start.byte as usize <= text.len());
                assert!(at.end.byte as usize <= text.len());
            }
        }
    }
});
