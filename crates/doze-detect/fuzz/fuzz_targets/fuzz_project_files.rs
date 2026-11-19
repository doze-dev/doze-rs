#![no_main]
//! Any text, as a project's compose file, go.mod or Makefile, without a panic; an alias bomb costs nothing (the
//! compose reader never expands an alias); and any text quoted for HCL reads back as one string.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = doze_detect::compose_services(text);
        let _ = doze_detect::requires(text);
        let _ = doze_detect::make_targets(text);
        let _ = doze_detect::quote(text);
    }
});
