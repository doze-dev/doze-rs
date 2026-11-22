#![no_main]
//! Any method and any three headers: never a panic, and a write is let through only as the rules say: a GET or HEAD
//! always; otherwise a same-origin fetch site, or an Origin of `http(s)://<authority>` whose authority is the Host.

use aws_console::origin::check;
use libfuzzer_sys::fuzz_target;

fn opt(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.is_empty())
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let mut lines = text.splitn(4, '\n');
    let method = lines.next().unwrap_or("");
    let origin = opt(lines.next());
    let site = opt(lines.next());
    let host = opt(lines.next());
    let ok = check(method, origin, site, host).is_ok();
    let read = method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD");
    if read {
        assert!(ok, "{text:?}");
        return;
    }
    if !ok {
        return;
    }
    if site.is_some_and(|s| s.trim().eq_ignore_ascii_case("same-origin")) {
        return;
    }
    // Let through without same-origin: the site said nothing, and the Origin names exactly the Host.
    assert!(site.is_none_or(|s| s.trim().is_empty()), "{text:?}");
    let o = origin
        .expect("a write let through carries an Origin")
        .trim();
    let authority = o
        .strip_prefix("http://")
        .or_else(|| o.strip_prefix("https://"))
        .expect("an http(s) origin");
    assert!(!authority.contains(['/', '@', '?', '#', ' ']), "{text:?}");
    assert!(
        authority.eq_ignore_ascii_case(host.expect("a Host").trim()),
        "{text:?}"
    );
});
