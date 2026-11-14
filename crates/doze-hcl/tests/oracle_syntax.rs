//! Syntax against Go: the token stream of every source (`oracle-dump lex`), and every diagnostic
//! Go's parser gives, in Go's order, with its range and summary (`oracle-dump check`).
//!
//! The corpus (`testdata/syntax/cases.json`) is 2,500 random sequences of HCL fragments, the real
//! sources and every eval-corpus expression as `x = <expr>`; regenerate Go's side with
//! `testdata/tools` (see its README).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_hcl::lex::lex;
use serde_json::Value as Json;

fn load(name: &str) -> Vec<Json> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/syntax");
    let text = std::fs::read_to_string(format!("{dir}/{name}")).unwrap();
    if name.ends_with(".jsonl") {
        text.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    } else {
        serde_json::from_str(&text).unwrap()
    }
}

fn tokens(src: &str) -> String {
    lex(src.as_bytes())
        .iter()
        .map(|t| {
            let (s, e) = (t.range.start, t.range.end);
            let name = t.kind.go_name();
            let name = name.trim_start_matches("Token");
            format!(
                "{name} {} {} {}:{}-{}:{}",
                t.start, t.end, s.line, s.column, e.line, e.column
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

#[test]
fn tokens_match_go() {
    let cases = load("cases.json");
    let go = load("go-lex.jsonl");
    assert_eq!(cases.len(), go.len());
    let mut diffs = Vec::new();
    for (c, g) in cases.iter().zip(&go) {
        assert_eq!(c["id"], g["id"]);
        let got = tokens(c["src"].as_str().unwrap());
        // Go's tokens are kept as a hash of the same text (testdata/tools/compact.ts); on a
        // difference, `oracle-dump lex` on the case shows them.
        if fnv(&got) != g["h"].as_str().unwrap() {
            diffs.push(format!("{}: {:?}\n   rust: {got}", c["id"], c["src"]));
        }
    }
    eprintln!(
        "lex: {} of {} identical",
        cases.len() - diffs.len(),
        cases.len()
    );
    assert!(
        diffs.is_empty(),
        "{} differ:\n{}",
        diffs.len(),
        diffs
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn diags(src: &str) -> Vec<String> {
    doze_hcl::check::check(src.as_bytes(), "x.hcl")
        .iter()
        .map(|d| {
            let r = d.subject.unwrap_or_default();
            format!(
                "{}:{}-{}:{} {}",
                r.start.line, r.start.column, r.end.line, r.end.column, d.summary
            )
        })
        .collect()
}

#[test]
fn diagnostics_match_go() {
    let cases = load("cases.json");
    let go = load("go-check.jsonl");
    assert_eq!(cases.len(), go.len());
    let mut diffs = Vec::new();
    for (c, g) in cases.iter().zip(&go) {
        assert_eq!(c["id"], g["id"]);
        let got = diags(c["src"].as_str().unwrap());
        let want: Vec<String> = g["d"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap().to_owned())
            .collect();
        if got != want {
            diffs.push(format!(
                "{}: {:?}\n   go:   {want:?}\n   rust: {got:?}",
                c["id"], c["src"]
            ));
        }
    }
    eprintln!(
        "check: {} of {} identical",
        cases.len() - diffs.len(),
        cases.len()
    );
    assert!(
        diffs.is_empty(),
        "{} differ:\n{}",
        diffs.len(),
        diffs
            .iter()
            .take(25)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// FNV-1a, 64 bits, as hex (testdata/tools/compact.ts).
fn fnv(s: &str) -> String {
    let h = s.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{h:016x}")
}

/// Identifiers are Go's Unicode 9.0 ID_Start / ID_Continue (swept from hclsyntax, every code point):
/// newer letters are not identifiers, and a few ID-but-not-XID characters are.
#[test]
fn identifiers_are_unicode_9_as_in_go() {
    let ident = |s: &str| {
        let t = lex(s.as_bytes());
        t.first()
            .is_some_and(|t| t.kind == doze_hcl::lex::Kind::Ident && t.end == s.len())
    };
    assert!(!ident("a\u{860}"), "U+0860 is Unicode 10");
    assert!(!ident("\u{560}"), "U+0560 is Unicode 11");
    assert!(ident("\u{37a}"), "U+037A is ID_Start, not XID_Start");
    assert!(ident("a\u{309b}"), "U+309B is ID_Continue");
    assert!(
        !ident("a\u{200d}"),
        "ZWJ is not an identifier character in Go"
    );
    assert!(ident("_a-b9é"));
}
