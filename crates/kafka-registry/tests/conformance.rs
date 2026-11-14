//! Conformance against Confluent's own registry: the scripts the Go edition recorded against cp-schema-registry 7.7.1
//! (`tests/data/conformance`, copied from doze-kafka's `schemaregistry/testdata/conformance`) are played against this
//! registry, which must give the same answers. Nothing here is written from memory of what Confluent does.
//!
//! Ids are global and keep counting, so each run first registers a throwaway schema to learn the next id (the base) and
//! removes it; the scripts write `@N` for base+N and the recordings hold ids as offsets from the base.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use kafka_registry::api::{Api, Body, Options};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct Step {
    method: String,
    path: String,
    #[serde(default)]
    body: String,
    #[serde(default, rename = "contentType")]
    content_type: String,
    #[serde(default)]
    auth: String,
    status: u16,
    response: Value,
    #[serde(default)]
    challenge: String,
}

/// The answers where this registry differs from Confluent's on purpose, as the Go edition lists them ("scenario/step").
const GO_DIFFERENCES: &[(&str, &str)] = &[
    (
        "modes/9",
        "IMPORT mode is not offered, so the list of valid modes does not name it (REG-D1).",
    ),
    (
        "readonly-subject/12",
        "IMPORT is refused as an invalid mode, 42204 (REG-D1).",
    ),
    (
        "readonly-subject/13",
        "IMPORT is not offered, forced or not (REG-D1).",
    ),
    (
        "readonly-subject/14",
        "READONLY_OVERRIDE is not offered (REG-D1).",
    ),
    (
        "readonly-subject/15",
        "Follows from steps 13 and 14 (REG-D1).",
    ),
    (
        "readonly-subject/16",
        "IMPORT is refused as an invalid mode (REG-D1).",
    ),
    (
        "readonly-subject/17",
        "IMPORT is not offered, forced or not (REG-D1).",
    ),
    ("readonly-subject/18", "Follows from step 17 (REG-D1)."),
    (
        "subject-names/3",
        "A subject up to 16 KiB is taken; Confluent's HTTP server refuses a path past 8 KiB (REG-D2).",
    ),
    (
        "subject-names/7",
        "Contexts are not offered: :.ctx:orders is a subject of that name (REG-D3).",
    ),
    (
        "subject-names/9",
        "Contexts are not offered: :*: is a subject of that name (REG-D3).",
    ),
    (
        "subject-names/12",
        "Contexts are not offered: GET /contexts is 404 (REG-D3).",
    ),
    (
        "large-bodies/2",
        "A body past 20 MiB is 413, not Confluent's parser's 400 (REG-D4).",
    ),
    (
        "large-bodies/3",
        "A config or mode body is at most 64 KiB (REG-D4).",
    ),
    (
        "large-bodies/4",
        "A config or mode body is at most 64 KiB (REG-D4).",
    ),
    (
        "large-bodies/5",
        "A config or mode body is at most 64 KiB (REG-D4).",
    ),
    (
        "unserved-routes-and-parameters/1",
        "Contexts are not offered (REG-D3).",
    ),
    (
        "unserved-routes-and-parameters/2",
        "The metadata endpoints are not served (REG-D5).",
    ),
    (
        "unserved-routes-and-parameters/3",
        "The metadata endpoints are not served (REG-D5).",
    ),
    (
        "unserved-routes-and-parameters/8",
        "format=serialized is ignored, where Confluent answers 500 (REG-D6).",
    ),
    (
        "unserved-routes-and-parameters/12",
        "A lookup in a soft-deleted subject is its 404, where Confluent answers 500 (REG-D6).",
    ),
    (
        "basic-authentication/8",
        "A PUT without credentials is 401 with the JSON error, not an empty body (REG-D7).",
    ),
    (
        "basic-authentication/9",
        "A DELETE without credentials is 401 with the JSON error (REG-D7).",
    ),
    (
        "unknown-id/1",
        "GET /schemas/ids/{unknown}/schema is 404 40403, where Confluent answers 500 (REG-D6).",
    ),
];

/// Steps that need a schema language this build does not read yet: they pass when its slice lands and is listed in
/// `Formats::all`, and are then taken off this list.
const PENDING: &[(&str, &str)] = &[
    ("canonical-forms/15", "JSON Schema: slice 10c."),
    ("canonical-forms/16", "JSON Schema: slice 10c."),
];

/// Whole recordings about a language this build does not read yet.
const PENDING_SCENARIOS: &[(&str, &str)] = &[
    ("nested-references", "JSON Schema references: slice 10c."),
    (
        "other-schema-types",
        "JSON Schema and Protobuf beside Avro: slices 10b and 10c (ids follow the JSON and Protobuf registrations).",
    ),
    ("protobuf-canonical-forms", "Protobuf: slice 10b."),
    ("protobuf-descriptor-with-import", "Protobuf: slice 10b."),
];

fn sub(s: &str, base: u64) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        if let Some(n) = after.strip_prefix("PAD(").and_then(|p| p.split_once(')')) {
            out.push_str(&"x".repeat(n.0.parse().unwrap()));
            rest = n.1;
            continue;
        }
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            out.push('@');
            rest = after;
        } else {
            out.push_str(&(base + digits.parse::<u64>().unwrap()).to_string());
            rest = &after[digits.len()..];
        }
    }
    out.push_str(rest);
    out
}

fn probe(api: &Api, auth: &str) -> u64 {
    let body = common::schema_body(r#"{"type":"record","name":"Probe1000","fields":[]}"#);
    let mut req = common::request("POST", "/subjects/zz-probe/versions", &body, "");
    req.authorization = basic(auth);
    let resp = api.serve(&req);
    assert_eq!(
        resp.status,
        200,
        "probe: {}",
        String::from_utf8_lossy(&resp.body)
    );
    for target in ["/subjects/zz-probe", "/subjects/zz-probe?permanent=true"] {
        let mut del = common::request("DELETE", target, "", "");
        del.authorization = basic(auth);
        let _ = api.serve(&del);
    }
    common::id_of(&String::from_utf8_lossy(&resp.body))
}

fn basic(auth: &str) -> Option<String> {
    if auth.is_empty() || auth == "none" {
        return None;
    }
    // Standard base64 of "user:password", written here so the test needs no crate.
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = auth.as_bytes();
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &c)| acc | (u32::from(c) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(T[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    Some(format!("Basic {out}"))
}

/// A long answer is compared as its error code and its ends, as it was recorded.
fn clip(body: &str) -> String {
    if body.len() <= 4096 {
        return body.to_owned();
    }
    let code = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error_code").cloned());
    let b = body.as_bytes();
    serde_json::json!({
        "clipped": body.len(),
        "error_code": code,
        "head": String::from_utf8_lossy(&b[..200]),
        "tail": String::from_utf8_lossy(&b[b.len() - 200..]),
    })
    .to_string()
}

/// The answer with ids as offsets from the base, as the recordings hold them.
fn relativize(path: &str, body: &str, base: u64) -> Value {
    let body = body.trim();
    let shift = |n: u64| n.wrapping_sub(base) as i64;
    if path.ends_with("/referencedby")
        && let Ok(ids) = serde_json::from_str::<Vec<u64>>(body)
    {
        return Value::from(ids.into_iter().map(shift).collect::<Vec<_>>());
    }
    let mut text = body.to_owned();
    for key in ["\"id\":", "\"maxId\":"] {
        let mut out = String::new();
        let mut rest = text.as_str();
        while let Some(i) = rest.find(key) {
            out.push_str(&rest[..i + key.len()]);
            let after = &rest[i + key.len()..];
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                rest = after;
                continue;
            }
            out.push_str(&shift(digits.parse().unwrap()).to_string());
            rest = &after[digits.len()..];
        }
        out.push_str(rest);
        text = out;
    }
    if let Some(i) = text.find("Schema ")
        && let Some(end) = text[i + 7..].find(" not found")
        && let Ok(n) = text[i + 7..i + 7 + end].parse::<u64>()
    {
        text = format!("{}Schema @{}{}", &text[..i], shift(n), &text[i + 7 + end..]);
    }
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

/// Compares a recorded answer with ours, as the Go edition does: the status and challenge exactly; an error by its code
/// and its message (up to ", details:" for the parser's own words, or only "Invalid schema"); anything else whole.
fn differ(want: &Step, status: u16, got: &Value, challenge: &str) -> Option<String> {
    if want.challenge != challenge {
        return Some(format!(
            "WWW-Authenticate {challenge:?}, Confluent sent {:?}",
            want.challenge
        ));
    }
    if want.status != status {
        return Some(format!(
            "status {status}, Confluent {}\n  ours:      {got}\n  Confluent: {}",
            want.status, want.response
        ));
    }
    if let (Some(wcode), Some(g)) = (want.response.get("error_code"), got.as_object()) {
        if Some(wcode) != g.get("error_code") {
            return Some(format!(
                "error_code differs\n  ours:      {got}\n  Confluent: {}",
                want.response
            ));
        }
        if wcode == 400 {
            return None;
        }
        let full = want.response["message"].as_str().unwrap_or_default();
        let mut wmsg = full.to_owned();
        let mut gmsg = g
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if let Some(i) = full.find(", details:")
            && wcode != 409
        {
            wmsg.truncate(i);
            if let Some(j) = gmsg.find(", details:") {
                gmsg.truncate(j);
            }
        }
        if full.contains("Invalid schema") {
            wmsg = "Invalid schema".into();
            if gmsg.starts_with("Invalid schema") {
                gmsg = wmsg.clone();
            }
        }
        return (wmsg != gmsg)
            .then(|| format!("message differs\n  ours:      {gmsg}\n  Confluent: {wmsg}"));
    }
    (want.response != *got).then(|| format!("\n  ours:      {got}\n  Confluent: {}", want.response))
}

fn play(name: &str, steps: &[Step]) -> Vec<String> {
    let auth = if name == "basic-authentication" {
        "alice:alice-secret"
    } else {
        ""
    };
    let mut options = Options {
        auth: None,
        allowed_hosts: common::test_hosts(),
    };
    if !auth.is_empty() {
        options.auth = Some(Arc::new(|u: &str, p: &str| {
            u == "alice" && p == "alice-secret"
        }));
    }
    let api = Api::new(common::registry(), options);
    let base = probe(&api, auth);
    let mut bad = Vec::new();
    for (n, st) in steps.iter().enumerate() {
        let key = format!("{name}/{n}");
        let (path, body) = (sub(&st.path, base), sub(&st.body, base));
        let mut req = common::request(&st.method, &path, &body, &st.content_type);
        req.authorization = basic(if st.auth.is_empty() { auth } else { &st.auth });
        req.body = Body::Read(body.into_bytes());
        let resp = api.serve(&req);
        if let Some((_, why)) = GO_DIFFERENCES
            .iter()
            .chain(PENDING)
            .find(|(k, _)| *k == key)
        {
            eprintln!("{key} differs on purpose: {why}");
            continue;
        }
        let text = clip(&String::from_utf8_lossy(&resp.body));
        let got = relativize(&path, &text, base);
        let challenge = if resp.challenge { "basic" } else { "" };
        if let Some(msg) = differ(st, resp.status, &got, challenge) {
            bad.push(format!(
                "{key}  {} {} {}\n  {msg}",
                st.method,
                st.path,
                st.body.chars().take(300).collect::<String>()
            ));
        }
    }
    bad
}

#[test]
fn every_recorded_scenario_answers_as_confluent_did() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/conformance");
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    let (mut played, mut bad) = (0, Vec::new());
    for path in names {
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        if let Some((_, why)) = PENDING_SCENARIOS.iter().find(|(k, _)| *k == name) {
            eprintln!("{name}: pending, {why}");
            continue;
        }
        let steps: Vec<Step> = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        played += steps.len();
        bad.extend(play(&name, &steps));
    }
    assert!(
        bad.is_empty(),
        "{} of {played} steps differ:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
