//! `doze add` and `doze init` against Go's. `tests/fixtures/go/add-init.json` is what Go's `doze` printed and wrote
//! (offline, `DOZE_MODULES=off`, nothing on the PATH): six `add` scenarios, and `init -y` over doze-detect's 121
//! oracle projects. Here the registry's modules declare exactly what Go had built in (its `kinds` table, seed versions
//! and ports: doze-detect's `tests/fixtures/oracle/kinds.json`, plus kvrocks), so the files must match Go's.
//!
//! Reviewed differences, applied to Go's side before comparing:
//! - `add`'s "use it from an app" line comes from the variables a module declares (url first), not Go's per-engine
//!   table, so it is left out of the comparison;
//! - a service whose name is not a label (`my_thing`) is labelled with the name `add` checked (`my-thing`); Go
//!   labelled it with the raw name;
//! - init: a module found in go.mod says "go.mod requires", and module blocks come in the modules' name order (Go's
//!   was its table's), so blocks are compared as a set, each byte for byte;
//! - init with nothing found writes a commented starter where Go wrote postgres (F6): those projects are skipped.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;

use doze_cli::Terminals;
use serde_json::Value;
use std::path::Path;

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The registry: Go's built-in module facts, declared by the modules.
fn registry(sb: &fixture::Sandbox) {
    let kinds: Value = serde_json::from_str(
        &std::fs::read_to_string(
            manifest().join("../doze-detect/tests/fixtures/oracle/kinds.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut list: Vec<Value> = kinds.as_array().unwrap().clone();
    list.push(
        serde_json::json!({"module": "kvrocks", "label": "kvrocks", "port": 6380, "version": "2",
        "category": "database", "go": [], "npm": [], "images": [], "env": {}}),
    );
    let reg = sb.registry();
    let mut names = Vec::new();
    for k in &list {
        let m = k["module"].as_str().unwrap();
        names.push(format!("\"{m}\":{{}}"));
        let dir = reg.join("doze").join(m);
        std::fs::create_dir_all(&dir).unwrap();
        // The engine majors Go's catalog gave: the seed version, or none (aws).
        let mut idx = fixture::index_for(m, &[("0.3.0", 2, "")]);
        let engines: Vec<String> = k["version"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(String::from)
            .into_iter()
            .collect();
        for r in idx.releases.values_mut() {
            r.engines = engines.clone();
        }
        let payload = doze_cli::registry::index::payload(&idx);
        idx.signature = fixture::sign_hex(
            &fixture::test_key(),
            &doze_cli::registry::signed::sha256_hex(payload.as_bytes()),
        );
        std::fs::write(dir.join("index.yaml"), fixture::index_text(&idx)).unwrap();
        let list_of = |key: &str| serde_json::to_string(&k[key]).unwrap();
        let env: Vec<String> = k["env"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(e, a)| format!("        {e}: {}", a.as_str().unwrap()))
            .collect();
        let mut meta = format!(
            "title: {m}\ntagline: {m}\ncategory: {}\nengine: {m}\nport: {}\nexample: x\nexampleLabel: {}\n",
            k["category"].as_str().unwrap(),
            k["port"],
            k["label"].as_str().unwrap()
        );
        if let Some(v) = k["version"].as_str().filter(|v| !v.is_empty()) {
            meta.push_str(&format!("defaultVersion: \"{v}\"\n"));
        }
        meta.push_str(&format!(
            "detect:\n    packages:\n        go: {}\n        npm: {}\n    images: {}\n",
            list_of("go"),
            list_of("npm"),
            list_of("images")
        ));
        if !env.is_empty() {
            meta.push_str(&format!("    env:\n{}\n", env.join("\n")));
        }
        std::fs::write(dir.join("meta.yaml"), meta).unwrap();
    }
    std::fs::write(
        reg.join("index.json"),
        format!(
            "{{\"namespaces\":{{\"doze\":{{\"modules\":{{{}}}}}}}}}",
            names.join(",")
        ),
    )
    .unwrap();
}

fn go() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(manifest().join("tests/fixtures/go/add-init.json")).unwrap(),
    )
    .unwrap()
}

/// Stdout without the wiring line (a reviewed difference) and blank lines.
fn comparable(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.contains("use it from an app:") && !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn add_writes_what_go_writes() {
    let mut differ = Vec::new();
    for case in go()["add"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let sb = fixture::Sandbox::new(&format!("goadd-{name}"));
        registry(&sb);
        let initial = case["initial"].as_str().unwrap();
        if !initial.is_empty() {
            std::fs::write(sb.project().join("doze.hcl"), initial).unwrap();
        }
        let host = fixture::FakeHost::default();
        for step in case["steps"].as_array().unwrap() {
            let args: Vec<&str> = step["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap())
                .collect();
            let ran = fixture::run(&sb, &host, Terminals::default(), true, &args);
            let want = step["stdout"]
                .as_str()
                .unwrap()
                .replace("service \"my_thing\"", "service \"my-thing\"");
            if comparable(&ran.stdout) != comparable(&want)
                || i64::from(ran.code) != step["exit"].as_i64().unwrap()
            {
                differ.push(format!(
                    "{name}: doze {}\n--- go (exit {})\n{want}--- rust (exit {})\n{}{}",
                    args.join(" "),
                    step["exit"],
                    ran.code,
                    ran.stdout,
                    ran.stderr
                ));
            }
        }
        let file = std::fs::read_to_string(sb.project().join("doze.hcl")).ok();
        // The sandbox's project folder is `project`; Go's was `proj`.
        let want = case["file"].as_str().map(|f| {
            f.replace("service \"my_thing\"", "service \"my-thing\"")
                .replace("stack = \"proj\"", "stack = \"project\"")
        });
        if file != want {
            differ.push(format!(
                "{name}: the file\n--- go\n{want:?}\n--- rust\n{file:?}"
            ));
        }
    }
    assert!(differ.is_empty(), "{}", differ.join("\n"));
}

/// A file as its blocks (split at blank lines), sorted: module order is the modules' names, not Go's table.
fn blocks(text: &str) -> Vec<String> {
    let mut b: Vec<String> = text
        .replace("# go.mod requires ", "# the code imports ")
        .split("\n\n")
        .map(|s| s.trim_end().to_owned())
        .collect();
    b.sort();
    b
}

#[test]
fn init_writes_what_go_writes() {
    let projects = manifest().join("../doze-detect/tests/fixtures/oracle/projects");
    let (mut compared, mut differ) = (0, Vec::new());
    for case in go()["init"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let Some(want) = case["file"].as_str() else {
            continue;
        };
        if !case["stdout"]
            .as_str()
            .unwrap()
            .contains("Found in this project")
        {
            continue; // Go found nothing and wrote its postgres starter (F6)
        }
        let sb = fixture::Sandbox::new(&format!("goinit-{name}"));
        registry(&sb);
        let dir = sb.root.join(name);
        copy(&projects.join(name), &dir);
        let host = fixture::FakeHost::default();
        let ran = fixture::run_in(
            &sb,
            &host,
            Terminals::default(),
            true,
            &dir,
            &["-y", "init"],
        );
        let got = std::fs::read_to_string(dir.join("doze.hcl")).unwrap_or_default();
        compared += 1;
        if ran.code != 0 || blocks(&got) != blocks(want) {
            differ.push(format!(
                "{name} (exit {}):\n--- go\n{want}\n--- rust\n{got}{}",
                ran.code, ran.stderr
            ));
        }
    }
    assert!(compared > 90, "compared {compared}");
    assert!(
        differ.is_empty(),
        "{} of {compared} differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dst = to.join(e.file_name());
        if e.path().is_dir() {
            copy(&e.path(), &dst);
        } else {
            std::fs::copy(e.path(), dst).unwrap();
        }
    }
}
