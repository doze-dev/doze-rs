#![no_main]
//! Any bytes, read as an extension bundle archive (raw tar, and gzip), a bundle.json, a control file and the toolchain
//! ledger, without a panic; every file the archive reader keeps stays inside the bundle; a ledger it reads writes back
//! and reads again as itself.

#[path = "../../src/tarball.rs"]
#[allow(dead_code)]
mod tarball;

#[path = "../../src/bundle_read.rs"]
#[allow(dead_code)]
mod bundle_read;

use libfuzzer_sys::fuzz_target;

fn inside(path: &str) -> bool {
    !path.starts_with('/') && !path.split('/').any(|p| p.is_empty() || p == "." || p == "..")
}

fuzz_target!(|data: &[u8]| {
    for items in [tarball::read_tar(data), tarball::read(data)].into_iter().flatten() {
        for it in items {
            assert!(inside(&it.path), "{}", it.path);
            if let tarball::Body::Link(t) = &it.body {
                assert!(!t.contains('/') && t != "..", "{t}");
            }
        }
    }
    let Ok(text) = std::str::from_utf8(data) else { return };
    let _ = bundle_read::Manifest::parse(text);
    let _ = bundle_read::default_version(text);
    let _ = bundle_read::compare(text, "1.0");
    if let Ok(l) = bundle_read::Ledger::parse(text) {
        assert_eq!(bundle_read::Ledger::parse(&l.encode()).ok(), Some(l));
    }
});
