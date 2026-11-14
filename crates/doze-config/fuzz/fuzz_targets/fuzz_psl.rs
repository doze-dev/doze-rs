//! Any text as a domain name: the public suffix lookup never panics, its suffix is a tail of the
//! name, and the registrable domain is a tail that ends with that suffix.
#![no_main]

use doze_config::psl::{effective_tld_plus_one, public_suffix};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(name) = std::str::from_utf8(data) else { return };
    let (suffix, _) = public_suffix(name);
    assert!(name.ends_with(suffix));
    if let Some(domain) = effective_tld_plus_one(name) {
        assert!(name.ends_with(domain) && domain.ends_with(suffix) && domain.len() > suffix.len());
    }
});
