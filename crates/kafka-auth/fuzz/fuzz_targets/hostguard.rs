#![no_main]
//! Any Host header, with and without a list: never a panic, and what is let in is what the rules say. With
//! no list only an address, `localhost` or a name under it gets through, and never text that could name someone
//! else (userinfo, a path, a space, a control character); a listed name gets through only as that name, with
//! its case, one trailing dot and a port.

use kafka_auth::hostguard::allowed;
use libfuzzer_sys::fuzz_target;

const LISTED: &str = "dev.example.com";

fn main_part(header: &str) -> &str {
    // What is left of a header once a numeric port is off its end.
    match header.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
        _ => header,
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(header) = std::str::from_utf8(data) else {
        return;
    };
    let none: [String; 0] = [];
    let list = [LISTED.to_owned()];
    let star = ["*".to_owned()];
    let plain = allowed(header, &none);
    if plain {
        assert!(header.is_ascii(), "{header:?}");
        assert!(
            !header.bytes().any(|b| b.is_ascii_control() || matches!(b, b' ' | b'@' | b'/' | b'\\' | b'#' | b'?' | b'%')),
            "{header:?}"
        );
        let host = main_part(header);
        let ok = host.starts_with('[')
            || host.parse::<std::net::Ipv4Addr>().is_ok()
            || host.trim_end_matches('.').to_ascii_lowercase().ends_with("localhost");
        assert!(ok, "{header:?}");
    }
    if allowed(header, &list) && !plain {
        let host = main_part(header).to_ascii_lowercase();
        assert!(host == LISTED || host == format!("{LISTED}."), "{header:?}");
    }
    // The list only ever adds.
    if plain {
        assert!(allowed(header, &list) && allowed(header, &star), "{header:?}");
    }
    // A star lets everything through but an empty host.
    if allowed(header, &star) {
        assert!(!main_part(header).is_empty() && header != "[]", "{header:?}");
    }
});
