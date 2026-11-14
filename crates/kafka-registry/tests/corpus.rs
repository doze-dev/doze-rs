//! The compatibility corpus the Go edition recorded from Confluent's registry: schema pairs (old, new) with Confluent's
//! verdict at BACKWARD, FORWARD and FULL (`tests/data/compat/avro.json`: 92 written by hand, 300 generated), and its
//! reasons, word for word, for every refusal (`avro-reasons.json`). This registry must give the same verdicts and the
//! same reasons, and its check must agree with its registration on every pair and level.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use kafka_registry::api::Api;
use kafka_registry::format::SchemaType;
use kafka_registry::registry::{Origin, RegisterResult, SchemaInput};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    old: String,
    new: String,
    #[serde(default)]
    backward: String,
    #[serde(default)]
    forward: String,
    #[serde(default)]
    full: String,
}

#[derive(Debug, Deserialize)]
struct CorpusFile {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct ReasonsFile {
    reasons: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

/// The languages whose corpus this build answers: all three.
const KINDS: &[SchemaType] = &[SchemaType::Avro, SchemaType::Protobuf, SchemaType::Json];

/// Pairs (kind/name) where this registry answers otherwise than Confluent on purpose, with the deviation.
const DIFFERENCES: &[(&str, &str)] = &[
    (
        "JSON/exclusiveMaximum increased",
        "Confluent answers 500 when exclusiveMaximum is lowered; this registry answers the comparison (REG-D18).",
    ),
    (
        "JSON/exclusiveMaximum decreased",
        "Confluent answers 500 when exclusiveMaximum is lowered; this registry answers the comparison (REG-D18).",
    ),
];

fn differs(kind: SchemaType, name: &str) -> bool {
    let key = format!("{}/{name}", kind.name());
    DIFFERENCES.iter().any(|(k, _)| *k == key)
}

fn data(file: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/compat")
            .join(file),
    )
    .unwrap()
}

fn corpus(kind: SchemaType) -> Vec<Case> {
    let f: CorpusFile =
        serde_json::from_str(&data(&format!("{}.json", kind.name().to_lowercase()))).unwrap();
    f.cases
}

const SUBJECT: &str = "zz-corpus";

/// Registers `old` at NONE, then asks whether `new` may follow it at `level`: "yes", "no", or "invalid"; with the
/// reasons of a "no" (the lines before the three closing ones).
fn verdict(api: &Api, kind: SchemaType, c: &Case, level: &str) -> (String, Vec<String>) {
    let cleanup = |api: &Api| {
        let _ = common::send(api, "DELETE", &format!("/subjects/{SUBJECT}"), "");
        let _ = common::send(
            api,
            "DELETE",
            &format!("/subjects/{SUBJECT}?permanent=true"),
            "",
        );
        let _ = common::send(api, "DELETE", &format!("/config/{SUBJECT}"), "");
    };
    cleanup(api);
    let _ = common::send(
        api,
        "PUT",
        &format!("/config/{SUBJECT}"),
        r#"{"compatibility":"NONE"}"#,
    );
    let mut out = ("invalid".to_owned(), Vec::new());
    let (st, _) = common::send(
        api,
        "POST",
        &format!("/subjects/{SUBJECT}/versions"),
        &common::typed_body(kind.name(), &c.old),
    );
    if st == 200 {
        let _ = common::send(
            api,
            "PUT",
            &format!("/config/{SUBJECT}"),
            &format!(r#"{{"compatibility":"{level}"}}"#),
        );
        let path = format!("/compatibility/subjects/{SUBJECT}/versions/latest?verbose=true");
        let (st, body) = common::send(api, "POST", &path, &common::typed_body(kind.name(), &c.new));
        if st == 200 {
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            if v["is_compatible"].as_bool().unwrap() {
                out.0 = "yes".into();
            } else {
                out.0 = "no".into();
                out.1 = v["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| m.as_str().unwrap().to_owned())
                    .filter(|m| {
                        !m.starts_with("{oldSchemaVersion:")
                            && !m.starts_with("{oldSchema:")
                            && !m.starts_with("{validateFields:")
                    })
                    .collect();
            }
        }
    }
    cleanup(api);
    out
}

#[test]
fn every_recorded_verdict_and_reason_is_confluents() {
    for &kind in KINDS {
        let reasons: ReasonsFile = serde_json::from_str(&data(&format!(
            "{}-reasons.json",
            kind.name().to_lowercase()
        )))
        .unwrap();
        let api = common::api();
        let cases = corpus(kind);
        let mut bad = Vec::new();
        let mut refusals = 0;
        for c in cases.iter().filter(|c| !differs(kind, &c.name)) {
            let want = [&c.backward, &c.forward, &c.full];
            for (level, want) in ["BACKWARD", "FORWARD", "FULL"].iter().zip(want) {
                let (got, why) = verdict(&api, kind, c, level);
                if &got != want {
                    bad.push(format!(
                        "{} at {level}: ours {got}, Confluent {want}",
                        c.name
                    ));
                    continue;
                }
                if let Some(rs) = reasons.reasons.get(&c.name).and_then(|m| m.get(*level)) {
                    refusals += 1;
                    if &why != rs {
                        bad.push(format!(
                            "{} at {level}:\n    ours:      {}\n    Confluent: {}",
                            c.name,
                            why.join("\n               "),
                            rs.join("\n               ")
                        ));
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "{kind:?}: {} of {} cases differ:\n  {}",
            bad.len(),
            cases.len(),
            bad.join("\n  ")
        );
        assert!(
            cases.len() >= 392 && refusals > 100,
            "{kind:?}: {} cases, {refusals} refusals with reasons",
            cases.len()
        );
    }
}

/// The console's check and a client's registration are one engine: for every pair and level, a check says compatible
/// exactly when registering succeeds, and gives the reasons the 409 gives.
#[test]
fn a_check_agrees_with_a_registration_on_every_pair() {
    for &kind in KINDS {
        for c in corpus(kind) {
            for level in ["BACKWARD", "FORWARD", "FULL", "FULL_TRANSITIVE"] {
                let r = common::registry();
                let old = SchemaInput::of(kind, &c.old);
                if r.register("s", &old, false, &Origin::default()).is_err() {
                    continue;
                }
                r.set_level("s", level).unwrap();
                let new = SchemaInput::of(kind, &c.new);
                let checked = r.check("s", None, &new, false, None);
                let registered = r.register("s", &new, false, &Origin::default());
                match (checked, registered) {
                    (Err(ce), Err(re)) => {
                        assert_eq!(ce.error_code, re.error_code, "{} at {level}", c.name)
                    }
                    (Err(ce), Ok(_)) => panic!(
                        "{} at {level}: the check refused ({ce}), the registration did not",
                        c.name
                    ),
                    (Ok((_, found)), Ok(RegisterResult::Id(_))) => {
                        assert!(found.is_empty(), "{} at {level}", c.name)
                    }
                    (Ok((_, found)), Err(re)) => {
                        let msgs: Vec<String> = found.iter().flat_map(|f| f.messages()).collect();
                        assert!(
                            !found.is_empty(),
                            "{} at {level}: the check said compatible, the registration refused: {re}",
                            c.name
                        );
                        assert!(
                            re.message
                                .contains(&format!("details: [{}]", msgs.join(", "))),
                            "{} at {level}",
                            c.name
                        );
                    }
                    (Ok(_), Ok(other)) => panic!("{} at {level}: {other:?}", c.name),
                }
            }
        }
    }
}
