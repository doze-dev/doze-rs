#![no_main]
//! Any text, read as a grant's object name, without a panic; a name that reads is exactly the name its role and
//! target render (so nothing but a name the module wrote reaches SQL).

#[path = "../../src/names.rs"]
#[allow(dead_code)]
mod names;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data)
        && let Some((role, on)) = names::grant(s)
    {
        assert_eq!(on.name(&role), s);
    }
});
