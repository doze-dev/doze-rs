use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kafka_wire::ResponseError;

use super::*;

/// Every `ResponseError` variant name the protocol crate has, with its code, found by asking it for each code.
fn protocol_names() -> BTreeMap<String, i16> {
    (-1..=200)
        .filter_map(ResponseError::try_from_code)
        .filter(|e| !matches!(e, ResponseError::Unknown(_)))
        .map(|e| (e.to_string(), e.code()))
        .collect()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The names of the `ResponseError` variants a source text mentions as `E::Name` or `ResponseError::Name`.
fn mentioned(text: &str, known: &BTreeMap<String, i16>) -> Vec<String> {
    let mut out = Vec::new();
    for prefix in ["ResponseError::", "E::"] {
        let mut rest = text;
        while let Some(at) = rest.find(prefix) {
            let before = rest[..at].chars().last();
            rest = &rest[at + prefix.len()..];
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let name: String = rest.chars().take_while(|c| c.is_alphanumeric()).collect();
            if known.contains_key(&name) {
                out.push(name);
            }
        }
    }
    out
}

fn camel_to_screaming(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

#[test]
fn every_error_the_broker_names_has_a_row() {
    let known = protocol_names();
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for dir in [
        "kafka-broker/src",
        "kafka-server/src",
        "kafka-wire/src",
        "kafka-auth/src",
    ] {
        rust_files(&crates.join(dir), &mut files);
    }
    let mut seen = std::collections::BTreeSet::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        seen.extend(mentioned(&text, &known));
    }
    assert!(
        seen.len() >= 55,
        "the scan found only {}: it is broken",
        seen.len()
    );
    let missing: Vec<_> = seen.iter().filter(|n| of(known[*n]).is_none()).collect();
    assert!(
        missing.is_empty(),
        "codes the broker serves with no row in the error table: {missing:?}"
    );
}

#[test]
fn the_scan_would_find_a_code_with_no_row() {
    // The completeness test is only worth having if it can fail: a name that is a real variant and has no row is found.
    let known = protocol_names();
    let found = mentioned(
        "return Err(ResponseError::InvalidFetchSize);\nlet x = E::NotEnoughReplicas;",
        &known,
    );
    assert_eq!(
        found,
        vec![
            "InvalidFetchSize".to_owned(),
            "NotEnoughReplicas".to_owned()
        ]
    );
    assert!(of(known["InvalidFetchSize"]).is_none());
}

#[test]
fn a_rows_name_and_code_are_the_protocol_crates_own() {
    let known = protocol_names();
    let by_code: BTreeMap<i16, &String> = known.iter().map(|(n, c)| (*c, n)).collect();
    for row in all() {
        let want = by_code
            .get(&row.code)
            .unwrap_or_else(|| panic!("code {} is not a protocol error", row.code));
        assert_eq!(row.name, camel_to_screaming(want), "code {}", row.code);
        assert!(!row.words.is_empty() && !row.plain.is_empty());
    }
}

#[test]
fn codes_are_unique_and_the_classes_are_what_the_boards_say() {
    let mut codes: Vec<i16> = all().map(|r| r.code).collect();
    codes.sort_unstable();
    let n = codes.len();
    codes.dedup();
    assert_eq!(n, codes.len(), "a code has two rows");
    assert_eq!(class(0), Class::Ok);
    assert_eq!(class(87), Class::Refused);
    assert_eq!(class(10), Class::Refused);
    assert_eq!(class(47), Class::Refused);
    assert_eq!(class(27), Class::Warn);
    assert_eq!(class(29), Class::Denied);
    assert_eq!(class(58), Class::Denied);
    assert_eq!(class(-1), Class::Bad);
    assert_eq!(class(9999), Class::Bad);
    assert_eq!(name(0), "NONE");
    assert_eq!(name(87), "INVALID_RECORD");
    assert_eq!(name(9999), "ERROR_9999");
}
