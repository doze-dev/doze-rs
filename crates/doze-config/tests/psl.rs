//! `doze_config::psl` against the `psl` crate (built from the same list revision) on every rule of
//! the list, names one and two labels under each, and random names made from the list's labels.
//!
//! `psl` and Go agree on everything compared here; what only Go does (an IP address is its own
//! suffix; no Unicode rules, since Go's table is punycode) is held by the unit tests in src/psl.rs
//! and the cases at the end.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test helpers

use doze_config::psl::{effective_tld_plus_one, public_suffix};

const LIST: &str = include_str!("../data/public_suffix_list.fc");

/// TLDs with rules under them but none of their own, not even a wildcard (found by `parent_only_tlds_are_listed`).
const PARENT_ONLY_TLDS: &[&str] = &["za"];

/// The rules as written in the list (`!city.kawasaki.jp`, `*.ck`), decoded from the embedded copy.
fn rules() -> Vec<String> {
    let mut out = Vec::new();
    let mut key = String::new();
    for line in LIST.lines() {
        if line.starts_with("//") || line.starts_with('=') {
            key.clear();
            continue;
        }
        let mut chars = line.chars();
        let shared = chars.next().and_then(|c| c.to_digit(36)).unwrap() as usize;
        key.truncate(shared);
        key.push_str(chars.as_str());
        out.push(key.split('.').rev().collect::<Vec<_>>().join("."));
    }
    out
}

fn check(name: &str, failures: &mut Vec<String>) {
    let (suffix, icann) = public_suffix(name);
    let theirs = psl::suffix(name.as_bytes()).expect("psl has a suffix for every name here");
    let their_suffix = std::str::from_utf8(theirs.as_bytes()).unwrap();
    // The one place psl and Go part: a TLD the list names only as a parent (`za`: there are
    // `co.za` and friends but no `za` rule) is ICANN to Go, whose generator makes every node ICANN
    // until a private rule ends on it; psl calls it unknown. Checked against Go (x/net v0.60.0).
    let parent_only_tld = theirs.typ().is_none() && PARENT_ONLY_TLDS.contains(&their_suffix);
    let their_icann = theirs.typ() == Some(psl::Type::Icann) || parent_only_tld;
    if (suffix, icann) != (their_suffix, their_icann) {
        failures.push(format!(
            "{name}: ours {suffix} {icann}, psl {their_suffix} {their_icann}"
        ));
    }
    let ours = effective_tld_plus_one(name);
    let theirs = psl::domain(name.as_bytes()).map(|d| std::str::from_utf8(d.as_bytes()).unwrap());
    if ours != theirs {
        failures.push(format!("{name}: eTLD+1 ours {ours:?}, psl {theirs:?}"));
    }
}

/// A small deterministic generator (xorshift), so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, from: &'a [String]) -> &'a str {
        &from[(self.next() % from.len() as u64) as usize]
    }
}

#[test]
fn every_rule_and_names_under_it_match_psl() {
    let rules = rules();
    assert!(
        rules.len() > 9_000,
        "the list decoded to {} rules",
        rules.len()
    );
    let mut failures = Vec::new();
    let mut names = 0;
    for rule in &rules {
        let base = rule.trim_start_matches('!').replace('*', "wild");
        for name in [
            base.clone(),
            format!("x.{base}"),
            format!("a.b.{base}"),
            format!("www.{base}"),
        ] {
            check(&name, &mut failures);
            names += 1;
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {names} names differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn random_names_from_the_lists_labels_match_psl() {
    let rules = rules();
    let mut labels: Vec<String> = rules
        .iter()
        .flat_map(|r| r.split('.'))
        .map(|l| l.trim_start_matches('!').replace('*', "q"))
        .collect();
    labels.extend(["www", "example", "cromulent", "a", "0", "x-y", "_srv"].map(String::from));
    labels.sort();
    labels.dedup();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut failures = Vec::new();
    for _ in 0..200_000 {
        let depth = 1 + rng.next() % 5;
        let name: Vec<&str> = (0..depth).map(|_| rng.pick(&labels)).collect();
        check(&name.join("."), &mut failures);
    }
    // Random names under real suffixes: one to three random labels on a rule.
    for _ in 0..100_000 {
        let rule = rng.pick(&rules).trim_start_matches('!').replace('*', "z");
        let extra = (rng.next() % 3) as usize;
        let mut name: Vec<&str> = (0..extra).map(|_| rng.pick(&labels)).collect();
        name.push(&rule);
        check(&name.join("."), &mut failures);
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn what_go_does_and_psl_does_not() {
    // netip.ParseAddr accepts these, so Go's suffix is the whole name and there is no eTLD+1.
    for ip in ["10.0.0.1", "::1", "::ffff:1.2.3.4", "fe80::1%en0"] {
        assert_eq!(public_suffix(ip), (ip, false), "{ip}");
        assert_eq!(effective_tld_plus_one(ip), None, "{ip}");
    }
    // Not addresses to Go (a leading zero, five parts, an empty zone): names under a TLD-less "*".
    assert_eq!(effective_tld_plus_one("010.0.0.1"), Some("0.1"));
    assert_eq!(effective_tld_plus_one("1.2.3.4.5"), Some("4.5"));
    // Go's table is punycode only: a Unicode rule never matches its Unicode spelling.
    assert_eq!(public_suffix("example.xn--p1ai"), ("xn--p1ai", true));
    assert_eq!(public_suffix("example.рф"), ("рф", false));
    // Case is compared as bytes, as Go does.
    assert_eq!(public_suffix("example.CO.UK"), ("UK", false));
    // Empty labels: Go's error.
    for bad in ["", ".com", "com.", "a..com"] {
        assert_eq!(effective_tld_plus_one(bad), None, "{bad:?}");
    }
}

#[test]
fn parent_only_tlds_are_listed() {
    let rules = rules();
    let all: std::collections::HashSet<&str> = rules.iter().map(String::as_str).collect();
    let mut tlds: Vec<&str> = rules
        .iter()
        .filter_map(|r| r.rsplit('.').next())
        .filter(|tld| !all.contains(tld) && !all.contains(format!("*.{tld}").as_str()))
        .collect();
    tlds.sort_unstable();
    tlds.dedup();
    assert_eq!(tlds, PARENT_ONLY_TLDS);
    assert_eq!(public_suffix("example.za"), ("za", true));
}
