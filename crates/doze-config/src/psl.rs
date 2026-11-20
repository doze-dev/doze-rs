//! The public suffix list, as Go's `golang.org/x/net/publicsuffix` matches it.
//!
//! The list is embedded front-coded (`data/public_suffix_list.fc`, about 78 KB; refreshed with
//! `data/refresh-psl.ts`) and decoded into a table on the first lookup, so a stack without app
//! domains never pays for it. The table is Go's trie flattened to a sorted map keyed by the
//! whole suffix (`kawasaki.jp`), with the same per-node facts Go's generator records: the node
//! type (a rule, an exception, or only a parent of other rules), the wildcard bit, the ICANN bit
//! and whether it has children. [`public_suffix`] walks labels from the right exactly as Go's
//! `PublicSuffix` loop does, quirks included (the wildcard is applied before the child lookup;
//! a name that parses as an IP address is its own suffix).

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::LazyLock;

/// The embedded list (generated; see `data/refresh-psl.ts`).
const LIST: &str = include_str!("../data/public_suffix_list.fc");

/// What a node of the trie is, as Go's generator decides it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A rule names this suffix.
    Normal,
    /// An exception rule (`!city.kawasaki.jp`): the suffix ends one label further right.
    Exception,
    /// Only a parent of other rules (or of a wildcard).
    ParentOnly,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    kind: Kind,
    wildcard: bool,
    icann: bool,
    children: bool,
}

impl Node {
    const fn parent() -> Self {
        // Go's `child` makes a node ICANN until a private rule ends on it.
        Self {
            kind: Kind::ParentOnly,
            wildcard: false,
            icann: true,
            children: false,
        }
    }
}

type Table = BTreeMap<Box<str>, Node>;

static TABLE: LazyLock<Table> = LazyLock::new(|| decode(LIST));

/// The list's version line (`2026-11-13_14-44-35_UTC`), for `doze version` and bug reports.
#[must_use]
pub fn list_version() -> &'static str {
    LIST.lines()
        .find_map(|l| l.strip_prefix("// VERSION: "))
        .unwrap_or("unknown")
}

// fuzz:parser fuzz_psl
/// Go's `publicsuffix.PublicSuffix`: the public suffix of `domain` and whether it is managed by
/// ICANN (false for private suffixes such as `github.io` and for TLDs the list does not know).
#[must_use]
pub fn public_suffix(domain: &str) -> (&str, bool) {
    if is_ip(domain) {
        return (domain, false);
    }
    let table = &*TABLE;
    let (suffix, icann) = walk(table, domain);
    if suffix == domain.len() {
        // No rule matched: the prevailing rule is "*", the last label.
        let last = domain.rfind('.').map_or(0, |d| d + 1);
        return (domain.get(last..).unwrap_or(domain), icann);
    }
    (domain.get(suffix..).unwrap_or(domain), icann)
}

/// Go's `publicsuffix.EffectiveTLDPlusOne`: the registrable domain (the public suffix plus one
/// label), or `None` where Go returns an error (an empty label, or nothing left of the suffix).
#[must_use]
pub fn effective_tld_plus_one(domain: &str) -> Option<&str> {
    if domain.starts_with('.') || domain.ends_with('.') || domain.contains("..") {
        return None;
    }
    let (suffix, _) = public_suffix(domain);
    if domain.len() <= suffix.len() {
        return None;
    }
    let i = domain.len() - suffix.len() - 1;
    let head = domain.get(..i)?;
    if domain.as_bytes().get(i) != Some(&b'.') {
        return None;
    }
    let start = head.rfind('.').map_or(0, |d| d + 1);
    domain.get(start..)
}

/// Go's loop: returns where the suffix starts (`domain.len()` when no rule set it) and the
/// ICANN bit.
fn walk(table: &Table, domain: &str) -> (usize, bool) {
    let mut s = domain;
    let mut suffix = domain.len();
    let (mut icann, mut icann_node, mut wildcard, mut children) = (false, false, false, true);
    loop {
        let dot = s.rfind('.');
        let label_start = dot.map_or(0, |d| d + 1);
        if wildcard {
            icann = icann_node;
            suffix = label_start;
        }
        if !children {
            break;
        }
        let Some(node) = domain.get(label_start..).and_then(|key| table.get(key)) else {
            break;
        };
        icann_node = node.icann;
        children = node.children;
        match node.kind {
            Kind::Normal => suffix = label_start,
            Kind::Exception => {
                suffix = s.len() + 1;
                break;
            }
            Kind::ParentOnly => {}
        }
        wildcard = node.wildcard;
        if !wildcard {
            icann = icann_node;
        }
        let Some(d) = dot else { break };
        s = s.get(..d).unwrap_or_default();
    }
    (suffix, icann)
}

/// Go's `netip.ParseAddr` accepts the name: an IPv4 or IPv6 address, IPv6 with a zone.
fn is_ip(domain: &str) -> bool {
    match domain.split_once('%') {
        Some((addr, zone)) => !zone.is_empty() && addr.parse::<Ipv6Addr>().is_ok(),
        None => domain.parse::<IpAddr>().is_ok(),
    }
}

/// Decodes the front-coded list into the trie's nodes (Go's generator, rule by rule).
fn decode(list: &str) -> Table {
    let mut table = Table::new();
    let mut icann = true;
    let mut key = String::new();
    for line in list.lines() {
        match line {
            "=icann" => {
                icann = true;
                key.clear();
            }
            "=private" => {
                icann = false;
                key.clear();
            }
            l if l.starts_with("//") || l.is_empty() => {}
            l => {
                let mut chars = l.chars();
                let shared = chars.next().and_then(|c| c.to_digit(36)).unwrap_or(0);
                key.truncate(usize::try_from(shared).unwrap_or(0));
                key.push_str(chars.as_str());
                add_rule(&mut table, &key, icann);
            }
        }
    }
    table
}

/// Adds one rule, given as its reversed labels (`jp.kawasaki.!city`).
fn add_rule(table: &mut Table, reversed: &str, icann: bool) {
    let mut labels: Vec<&str> = reversed.split('.').collect();
    let (kind, wildcard) = match labels.last().copied() {
        Some("*") => {
            labels.pop();
            (Kind::ParentOnly, true)
        }
        Some(l) if l.starts_with('!') => {
            if let Some(last) = labels.last_mut() {
                *last = l.trim_start_matches('!');
            }
            (Kind::Exception, false)
        }
        _ => (Kind::Normal, false),
    };
    let mut name = String::new();
    for (depth, label) in labels.iter().enumerate() {
        if depth > 0 {
            if let Some(parent) = table.get_mut(name.as_str()) {
                parent.children = true;
            }
            name.insert(0, '.');
        }
        name.insert_str(0, label);
        let node = table
            .entry(name.as_str().into())
            .or_insert_with(Node::parent);
        if depth + 1 == labels.len() {
            if kind != Kind::ParentOnly && node.kind == Kind::ParentOnly {
                node.kind = kind;
            }
            node.icann = node.icann && icann;
            node.wildcard = node.wildcard || wildcard;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_examples() {
        // From x/net/publicsuffix's own tests.
        assert_eq!(public_suffix("foo.org"), ("org", true));
        assert_eq!(public_suffix("foo.co.uk"), ("co.uk", true));
        assert_eq!(public_suffix("foo.dyndns.org"), ("dyndns.org", false));
        assert_eq!(public_suffix("cromulent"), ("cromulent", false));
        assert_eq!(public_suffix("www.ck"), ("ck", true));
        assert_eq!(public_suffix("a.b.ck"), ("b.ck", true));
        assert_eq!(public_suffix("city.kawasaki.jp"), ("kawasaki.jp", true));
        assert_eq!(public_suffix("x.city.kawasaki.jp"), ("kawasaki.jp", true));
        assert_eq!(public_suffix("x.y.kawasaki.jp"), ("y.kawasaki.jp", true));
        assert_eq!(public_suffix("127.0.0.1"), ("127.0.0.1", false));
        assert_eq!(public_suffix("fe80::1%en0"), ("fe80::1%en0", false));
        assert_eq!(
            effective_tld_plus_one("a.b.example.co.uk"),
            Some("example.co.uk")
        );
        assert_eq!(effective_tld_plus_one("co.uk"), None);
        assert_eq!(effective_tld_plus_one("github.io"), None);
        assert_eq!(effective_tld_plus_one("me.github.io"), Some("me.github.io"));
        assert_eq!(effective_tld_plus_one("127.0.0.1"), None);
        assert_eq!(effective_tld_plus_one(""), None);
        assert_eq!(effective_tld_plus_one("a..com"), None);
        assert!(list_version().starts_with("20"));
    }
}
