//! The allowed-hosts guard: which `Host` headers an HTTP listener of this broker answers to.
//!
//! It is the defence against DNS rebinding. The console (Phase 9) is unauthenticated on loopback, and a web page
//! cannot read it directly; but a page whose own DNS name is answered first with the attacker's server and then
//! with `127.0.0.1` can, because the browser sees one origin all along. What the page cannot do is make the
//! browser send a `Host` that names something the attacker does not control. So a listener answers only to
//! names it expects. An IP literal can never be a rebinding name (there is no lookup to subvert), which is why
//! those are always allowed and reaching the console by a LAN address keeps working.
//!
//! This guards the HTTP listeners only: Kafka's own protocol has no `Host` header. Nothing is mounted on it yet.
//! Unlike the Go edition there is no special case for the `.doze` zone: a name the operator needs is listed.
//!
//! The rules, all of them strict because a refusal costs a flag and an acceptance costs the defence:
//! - An IPv4 literal, an IPv6 literal in brackets (`[::1]`), `localhost` and any name under it are allowed, and so
//!   is any name in the operator's list; nothing else is. `*` in the list allows any non-empty host.
//! - A port is only trailing digits after the last colon (possibly none: `localhost:`). Anything else there is
//!   part of what the header names (`localhost:pw@evil.example` is not localhost) and is refused.
//! - Brackets are the pair around the whole host and hold an IPv6 literal. `::1` without them is refused: the
//!   colons would be a port.
//! - A name is ASCII letters, digits, `-` and `_` in non-empty labels of at most 63 bytes, 253 in all, and
//!   compared without case. Non-ASCII is refused (a browser sends punycode), and a very long host is refused
//!   whatever the list holds.
//! - One trailing dot on a name is the same name (`localhost.`); a second makes it nobody's. An address with a
//!   trailing dot is not an address.
//! - An empty host is refused, even with `*`.

use std::net::{Ipv4Addr, Ipv6Addr};

const LOCALHOST: &str = "localhost";
const MAX_NAME: usize = 253;
const MAX_LABEL: usize = 63;

// fuzz:parser hostguard
/// Whether a request whose `Host` header is `host_header` may be answered. `extra` holds the operator's
/// additional names (`--console-allow-host`); a `*` in it, anywhere, allows any non-empty host.
#[must_use]
pub fn allowed(host_header: &str, extra: &[String]) -> bool {
    let Some((host, bracketed)) = split(host_header) else {
        return false;
    };
    let any = || extra.iter().any(|e| e.trim() == "*");
    if bracketed {
        return host.parse::<Ipv6Addr>().is_ok() || (!host.is_empty() && any());
    }
    if host.parse::<Ipv4Addr>().is_ok() {
        return true;
    }
    let name = host.strip_suffix('.').unwrap_or(host);
    if name.is_empty() {
        return false;
    }
    if any() {
        return true;
    }
    is_name(name) && (is_localhost(name) || extra.iter().any(|e| listed(e, name)))
}

/// The host of a header without its port, and whether it was written in brackets; `None` for a port that is not
/// digits or text after a closing bracket.
fn split(header: &str) -> Option<(&str, bool)> {
    if let Some(rest) = header.strip_prefix('[') {
        let (inside, after) = rest.split_once(']')?;
        let port = after.strip_prefix(':').unwrap_or(after);
        return (after.is_empty() || (after.starts_with(':') && digits(port)))
            .then_some((inside, true));
    }
    match header.rsplit_once(':') {
        Some((host, port)) => digits(port).then_some((host, false)),
        None => Some((header, false)),
    }
}

fn digits(port: &str) -> bool {
    port.bytes().all(|b| b.is_ascii_digit())
}

/// A DNS name: labels of ASCII letters, digits, `-` and `_`, none empty or over 63 bytes, 253 bytes at most.
fn is_name(name: &str) -> bool {
    name.len() <= MAX_NAME
        && name.split('.').all(|label| {
            (1..=MAX_LABEL).contains(&label.len())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// `localhost`, or a name under it (`app.localhost`); `name` is a valid name.
fn is_localhost(name: &str) -> bool {
    let at = name.len().saturating_sub(LOCALHOST.len());
    name.get(at..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case(LOCALHOST))
        && (at == 0 || name.get(..at).is_some_and(|head| head.ends_with('.')))
}

/// Whether one entry of the operator's list is `name` (case and one trailing dot ignored). An entry that is not a
/// name (it holds a port, a bracket, a wildcard pattern) matches nothing, as `name` is one.
fn listed(entry: &str, name: &str) -> bool {
    let entry = entry.trim();
    entry
        .strip_suffix('.')
        .unwrap_or(entry)
        .eq_ignore_ascii_case(name)
}

#[cfg(test)]
mod tests;
