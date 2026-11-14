//! Any bytes: formatting never panics and changes only spaces and tabs between tokens; editing a
//! file that opens never panics.
#![no_main]

use doze_hcl::edit::Edit;
use doze_hcl::fmt::format;
use libfuzzer_sys::fuzz_target;

fn without_blanks(b: &[u8]) -> Vec<u8> {
    b.iter().copied().filter(|c| *c != b' ' && *c != b'\t').collect()
}

fuzz_target!(|data: &[u8]| {
    let out = format(data);
    assert_eq!(without_blanks(&out), without_blanks(data));
    if let Ok(text) = std::str::from_utf8(data)
        && let Ok(mut e) = Edit::open(text)
    {
        let _ = e.blocks();
        let _ = e.append_block("app \"x\" {\n  run = \"y\"\n}\n");
        e.set_top_attribute("stack", "s\"${x}%{y}\n");
        let _ = e.bytes();
    }
});
