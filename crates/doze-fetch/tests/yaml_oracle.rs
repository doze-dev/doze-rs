//! The lock writer against Go's yaml.v3 encoder. The fixtures were written by a Go program that marshals Go's own
//! `binaries.Lock` struct (and plain string maps) with yaml.v3 v3.0.1, the version the Go doze builds with: 1,320
//! scalars, 215 keys in order, and whole locks. Every case must come out byte for byte.

use doze_fetch::lock::{self, EnginePin, Lock, Pin};
use doze_fetch::yaml_out::{key_order, scalar};
use serde_json::Value;
use std::collections::BTreeMap;

const SCALARS: &str = include_str!("fixtures/yaml/scalars.json");
const KEYS: &str = include_str!("fixtures/yaml/keys.json");

#[test]
fn every_scalar_is_written_as_go_writes_it() {
    let cases: Vec<(String, String)> = serde_json::from_str(SCALARS).unwrap();
    assert!(cases.len() > 1000);
    let wrong: Vec<_> = cases
        .iter()
        .filter(|(s, go)| format!("{}: {}\n", scalar(s), scalar(s)) != *go)
        .map(|(s, go)| format!("{s:?}: go {go:?}, rust {:?}", scalar(s)))
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} differ:\n{}",
        wrong.len(),
        cases.len(),
        wrong.join("\n")
    );
}

#[test]
fn keys_are_in_go_order() {
    let v: Value = serde_json::from_str(KEYS).unwrap();
    let mut keys: Vec<String> = serde_json::from_value(v["input"].clone()).unwrap();
    let go: Vec<String> = serde_json::from_value(v["go"].clone()).unwrap();
    keys.sort_by(|a, b| key_order(a, b));
    let rust: Vec<String> = keys.iter().map(|k| format!("{}: 0", scalar(k))).collect();
    assert_eq!(rust, go);
}

fn strings(v: &Value) -> BTreeMap<String, String> {
    v.as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn lock_of(v: &Value) -> Lock {
    let mut l = Lock {
        doze: v["doze"].as_str().unwrap_or("").into(),
        ..Lock::default()
    };
    for (eng, specs) in v["engines"].as_object().into_iter().flatten() {
        let e = l.engines.entry(eng.clone()).or_default();
        for (spec, p) in specs.as_object().into_iter().flatten() {
            let pin = EnginePin {
                resolved: p["resolved"].as_str().unwrap_or("").into(),
                source: p["source"].as_str().unwrap_or("").into(),
                hashes: strings(&p["hashes"]),
            };
            e.insert(spec.clone(), pin);
        }
    }
    for (src, p) in v["modules"].as_object().into_iter().flatten() {
        let pin = Pin {
            version: p["version"].as_str().unwrap_or("").into(),
            protocol: p["protocol"].as_i64().unwrap_or(0),
            engines: p["engines"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e.as_str().map(Into::into))
                .collect(),
            hashes: strings(&p["hashes"]),
            doze: p["doze"].as_str().unwrap_or("").into(),
        };
        l.modules.insert(src.clone(), pin);
    }
    for (url, p) in v["downloads"].as_object().into_iter().flatten() {
        l.downloads
            .insert(url.clone(), p["sha256"].as_str().unwrap_or("").into());
    }
    l
}

#[test]
fn whole_locks_are_written_as_go_writes_them_and_read_back_the_same() {
    for name in ["empty", "full", "modules-only"] {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/locks/");
        let json: Value =
            serde_json::from_str(&std::fs::read_to_string(format!("{dir}{name}.json")).unwrap())
                .unwrap();
        let go = std::fs::read_to_string(format!("{dir}{name}.lock")).unwrap();
        let built = lock_of(&json);
        assert_eq!(lock::render(&built), go, "{name}: written");
        let read = lock::parse(&go).unwrap();
        assert_eq!(lock::render(&read), go, "{name}: read back and written");
        assert_eq!(
            Lock {
                dirty: false,
                ..read
            },
            built,
            "{name}: read back"
        );
    }
}
