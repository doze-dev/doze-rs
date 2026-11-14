//! doze-config against Go's `internal/config`: every source in `testdata/oracle/files`, loaded in
//! both modes, gives what Go gave (`go-config.jsonl`, `go-shape.jsonl`; see
//! `testdata/tools/README.md`).
//!
//! Compared: the loaded config as Go's JSON (null, `[]` and `{}` as one; `Deps` and `Refs` sorted,
//! Go collects them in a map's order), or the refusal's kind and its diagnostics as a set (Go
//! reports several in a map's order), or, for a refusal without diagnostics, its text.
//!
//! Unix only: Go's answers were captured on Unix, so they hold Unix paths and Unix errors (`is a
//! directory`); Windows has its own gate in phase W.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use doze_config::{Options, load, load_shape};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use support::{Fixtures, OracleHost, config_json, normalize};

/// Sources where we differ from Go on purpose, by mode and file, and why. Each must differ.
const KNOWN: &[(&str, &str, &str)] = &[
    (
        "multi",
        "log-size",
        "a log_size past 2^63 bytes is refused; Go wraps it negative (differences.rs 7)",
    ),
    ("multi-shape", "log-size", "the same, in the shape pass"),
];

fn oracle_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/oracle")
}

fn mask(s: &str, home: &str, dir: &str, slug: &str) -> String {
    let out = s.replace(home, "<home>").replace(dir, "<dir>");
    let mut parts = out.split(slug);
    let mut masked = parts.next().unwrap_or("").to_owned();
    for p in parts {
        masked.push_str(slug);
        match p
            .get(..8)
            .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            Some(_) => {
                masked.push_str("<slug>");
                masked.push_str(p.get(8..).unwrap_or(""));
            }
            None => masked.push_str(p),
        }
    }
    masked
}

fn pos(p: doze_hcl::diag::Pos) -> Json {
    json!({"line": p.line, "col": p.column, "byte": p.byte})
}

fn diags_json(e: &doze_config::Error) -> Vec<Json> {
    let base = |f: Option<&str>| {
        Path::new(f.unwrap_or(""))
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    e.diags
        .iter()
        .map(|d| {
            let mut m = json!({
                "sev": "error", "summary": d.summary, "detail": d.detail,
            });
            if let Some(r) = d.subject {
                m["at"] = json!({"file": base(d.file.as_deref()), "start": pos(r.start), "end": pos(r.end)});
            }
            if let Some(r) = d.context {
                m["context"] = json!({"file": base(d.file.as_deref()), "start": pos(r.start), "end": pos(r.end)});
            }
            m
        })
        .collect()
}

/// A diagnostic as text with its keys sorted, whether or not serde_json keeps insertion order (a
/// workspace build turns on `preserve_order` for another crate; a `-p doze-config` build does not).
fn canon(j: &Json) -> String {
    fn sorted(j: &Json) -> Json {
        match j {
            Json::Object(o) => {
                let mut keys: Vec<_> = o.iter().collect();
                keys.sort_by(|a, b| a.0.cmp(b.0));
                Json::Object(
                    keys.into_iter()
                        .map(|(k, v)| (k.clone(), sorted(v)))
                        .collect(),
                )
            }
            Json::Array(a) => Json::Array(a.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    sorted(j).to_string()
}

/// Go's answer without what we do not compare (`err` when there are diagnostics).
fn go_side(mut g: Json) -> Json {
    let has_diags = g
        .get("diags")
        .and_then(Json::as_array)
        .is_some_and(|d| !d.is_empty());
    if let Some(o) = g.as_object_mut() {
        o.remove("id");
        if has_diags {
            o.remove("err");
        }
        if let Some(c) = o.remove("config") {
            o.insert("config".into(), normalize(c));
        }
        if let Some(Json::Array(ds)) = o.remove("diags") {
            let set: BTreeSet<String> = ds.iter().map(canon).collect();
            o.insert("diags".into(), json!(set));
        }
    }
    g
}

fn rust_side(
    r: Result<doze_config::Config, doze_config::Error>,
    home: &str,
    dir: &str,
    slug: &str,
) -> Json {
    let text = match r {
        Ok(c) => json!({"ok": true, "config": normalize(config_json(&c))}),
        Err(e) => {
            let ds = diags_json(&e);
            let set: BTreeSet<String> = ds.iter().map(canon).collect();
            let mut j = json!({"ok": false, "kind": if e.is_invalid() { "config_invalid" } else { "failed" }, "diags": set});
            if ds.is_empty() {
                j["err"] = json!(e.text);
            }
            j
        }
    };
    serde_json::from_str(&mask(&text.to_string(), home, dir, slug)).unwrap_or(Json::Null)
}

/// One corpus: its folder of sources, Go's answers, and the slug prefix of its data folder.
struct Corpus {
    folder: &'static str,
    answers: &'static str,
    slug: &'static str,
}

fn run(mode: &str, shape: bool, c: &Corpus) -> (usize, Vec<(String, Json, Json)>) {
    let td = oracle_dir();
    let files = td.join(c.folder);
    let home =
        std::env::temp_dir().join(format!("doze-config-oracle-{}-{mode}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let home = home.canonicalize().unwrap();
    let files = files.canonicalize().unwrap();
    let host = OracleHost {
        home: home.clone(),
        cwd: files.clone(),
        zone: None,
    };
    let opts = Options {
        host: &host,
        modules: &Fixtures,
        vars: Default::default(),
        version: "dev".into(),
    };
    let lines = std::fs::read_to_string(td.join(c.answers)).unwrap();
    let (h, d) = (
        home.to_string_lossy().into_owned(),
        files.to_string_lossy().into_owned(),
    );
    let mut diffs = Vec::new();
    for line in lines.lines() {
        let g: Json = serde_json::from_str(line).unwrap();
        let id = g["id"].as_str().unwrap().to_owned();
        let r = if shape {
            load_shape(&id, &opts)
        } else {
            load(&id, &opts)
        };
        let ours = rust_side(r, &h, &d, c.slug);
        let theirs = go_side(g);
        if ours != theirs {
            diffs.push((id, theirs, ours));
        }
    }
    let _ = std::fs::remove_dir_all(&home);
    (lines.lines().count(), diffs)
}

fn check(mode: &str, shape: bool, c: &Corpus) {
    let (total, diffs) = run(mode, shape, c);
    let known: BTreeSet<&str> = KNOWN
        .iter()
        .filter(|(m, ..)| *m == mode)
        .map(|(_, id, _)| *id)
        .collect();
    let mut report = String::new();
    for (id, go, rust) in &diffs {
        if !known.contains(id.as_str()) {
            report.push_str(&format!("\n== {id}\n go:   {go}\n rust: {rust}\n"));
        }
    }
    let differing: BTreeSet<&str> = diffs.iter().map(|(id, ..)| id.as_str()).collect();
    let stale: Vec<&&str> = known.iter().filter(|k| !differing.contains(**k)).collect();
    let unknown = diffs.len()
        - diffs
            .iter()
            .filter(|(id, ..)| known.contains(id.as_str()))
            .count();
    eprintln!(
        "{mode}: {} of {total} match Go ({} known differences)",
        total - diffs.len(),
        known.len()
    );
    assert!(
        unknown == 0 && stale.is_empty(),
        "{mode}: {unknown} differ from Go; known but now equal: {stale:?}{report}"
    );
}

const FILES: Corpus = Corpus {
    folder: "files",
    answers: "go-config.jsonl",
    slug: "files-",
};
const FILES_SHAPE: Corpus = Corpus {
    answers: "go-shape.jsonl",
    ..FILES
};
/// Stacks of several files, each loaded as a folder (overrides, siblings, auto vars).
const MULTI: Corpus = Corpus {
    folder: "multi",
    answers: "go-multi.jsonl",
    slug: "multi-",
};
const MULTI_SHAPE: Corpus = Corpus {
    answers: "go-multi-shape.jsonl",
    ..MULTI
};

#[test]
fn config_mode_matches_go() {
    check("config", false, &FILES);
}

#[test]
fn shape_mode_matches_go() {
    check("shape", true, &FILES_SHAPE);
}

#[test]
fn folders_match_go() {
    check("multi", false, &MULTI);
}

#[test]
fn folders_match_go_in_the_shape_pass() {
    check("multi-shape", true, &MULTI_SHAPE);
}
