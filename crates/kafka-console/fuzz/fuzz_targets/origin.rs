#![no_main]
//! Any method and any three headers: never a panic, a read always passes, and a change passes only on the browser's
//! word `same-origin` or an `http(s)` Origin whose authority equals the Host, case apart. The input is a presence byte
//! (bit 0 Origin, bit 1 Sec-Fetch-Site, bit 2 Host) and then the method, the origin, the site and the host, split at
//! NUL bytes.

use kafka_console::origin::check;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&present, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    let mut parts = text.split('\0');
    let method = parts.next().unwrap_or("");
    let pick = |bit: u8, part: Option<&str>| (present & bit != 0).then(|| part.unwrap_or(""));
    let origin = pick(1, parts.next());
    let site = pick(2, parts.next());
    let host = pick(4, parts.next());
    let got = check(method, origin, site, host);
    if method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD") {
        assert!(got.is_ok(), "{method:?}");
        return;
    }
    let same_origin = site.is_some_and(|s| s.trim().eq_ignore_ascii_case("same-origin"));
    let named_site = site.is_some_and(|s| !s.trim().is_empty());
    if named_site && !same_origin {
        assert!(got.is_err(), "{site:?}");
    }
    if got.is_ok() && !same_origin {
        let origin = origin.unwrap_or("").trim();
        let authority = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .unwrap_or("\0");
        let host = host.unwrap_or("").trim();
        assert!(authority.eq_ignore_ascii_case(host) && !host.is_empty(), "{origin:?} {host:?}");
        assert!(!authority.contains(['/', '@', '?', '#', ' ', '\\', '%']), "{origin:?}");
    }
});
