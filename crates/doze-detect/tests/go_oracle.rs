//! doze-detect against Go's `internal/detect`. `tests/fixtures/oracle/projects` holds 120 generated projects (go.mod,
//! main packages, package.json, compose files, Makefiles, env files) and a copy of the doze repo's `examples/shop`;
//! `go.json` is what Go's `detect.Scan` + `Scaffold` wrote for each (versions postgres 18, valkey 9, kafka 4,
//! mariadb 11; nothing on the PATH), recorded through `go build -overlay`, so the doze checkout is never touched.
//!
//! The modules here declare exactly what Go's engine table said (`kinds` in detect.go), so the files must be the same
//! byte for byte, with one reviewed difference: a module found in go.mod says "go.mod requires …" where Go said "the
//! code imports …" (Go's words were wrong for a go.mod line).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use doze_detect::{Declared, scaffold, scan};
use doze_module_proto::msg::hints::Detect;
use std::collections::BTreeMap;
use std::path::Path;

/// What Go's `kinds` table said (detect.go), as the five modules would declare it, in the table's order
/// (tests/fixtures/oracle/kinds.json).
pub fn go_kinds() -> Vec<Declared> {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("kinds.json")).unwrap())
            .unwrap();
    let list = |k: &serde_json::Value| -> Vec<String> {
        k.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_owned())
            .collect()
    };
    v.as_array()
        .unwrap()
        .iter()
        .map(|k| Declared {
            module: k["module"].as_str().unwrap().into(),
            label: k["label"].as_str().unwrap().into(),
            port: u16::try_from(k["port"].as_u64().unwrap()).unwrap(),
            version: k["version"].as_str().unwrap().into(),
            category: k["category"].as_str().unwrap().into(),
            detect: Detect {
                packages: BTreeMap::from([
                    ("go".into(), list(&k["go"])),
                    ("npm".into(), list(&k["npm"])),
                ]),
                images: list(&k["images"]),
                env: k["env"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(e, a)| (e.clone(), a.as_str().unwrap().to_owned()))
                    .collect(),
            },
        })
        .collect()
}

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oracle")
}

#[test]
fn every_project_scaffolds_as_go_does() {
    let want: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("go.json")).unwrap())
            .unwrap();
    assert!(want.len() > 100, "the corpus is there");
    let kinds = go_kinds();
    let mut differ = Vec::new();
    for (name, go) in &want {
        let found = scan(&fixtures().join("projects").join(name), &kinds);
        let rust = if found.is_empty() {
            String::new()
        } else {
            scaffold(&found, &|_| String::new())
                .replace("# go.mod requires ", "# the code imports ")
        };
        if &rust != go {
            differ.push(format!("{name}:\n--- go\n{go}\n--- rust\n{rust}"));
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {} differ:\n{}",
        differ.len(),
        want.len(),
        differ.join("\n")
    );
}

/// Go's TestTheShopFixtureIsRecognised: three apps, api wired to db, cache and events, four modules.
#[test]
fn the_shop_is_recognised() {
    let r = scan(&fixtures().join("projects/shop"), &go_kinds());
    assert_eq!(r.apps.len(), 3);
    let api = r.apps.iter().find(|a| a.name == "api").unwrap();
    assert_eq!(api.run, "go run ./cmd/api");
    let uses: BTreeMap<&str, &str> = api
        .uses
        .iter()
        .map(|u| (u.env.as_str(), u.kind.as_str()))
        .collect();
    assert_eq!(
        uses,
        BTreeMap::from([
            ("CACHE_ADDR", "valkey"),
            ("DATABASE_URL", "postgres"),
            ("KAFKA_BROKERS", "kafka")
        ])
    );
    let modules: Vec<&str> = r.modules.iter().map(|m| m.module.as_str()).collect();
    assert_eq!(modules, ["postgres", "valkey", "kafka", "aws"]);
}

/// A module that declares nothing is never guessed: with no declarations, the shop has apps and no modules.
#[test]
fn nothing_declared_nothing_guessed() {
    let r = scan(&fixtures().join("projects/shop"), &[]);
    assert_eq!((r.apps.len(), r.modules.len()), (3, 0));
    assert!(r.apps.iter().all(|a| a.uses.is_empty()));
}

/// Proposal F6: a go.mod with a one-line require of go-redis and no main package. Go found nothing (and init then
/// wrote postgres); here the requirement is read.
#[test]
fn a_one_line_require_is_read() {
    let d = std::env::temp_dir().join(format!("doze-detect-f6-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("go.mod"),
        "module x\n\ngo 1.22\n\nrequire github.com/redis/go-redis/v9 v9.5.1\n",
    )
    .unwrap();
    let r = scan(&d, &go_kinds());
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(r.modules.len(), 1);
    assert_eq!(
        r.modules.first().unwrap().why,
        "go.mod requires github.com/redis/go-redis/v9"
    );
}

#[test]
fn an_empty_folder_suggests_nothing() {
    let d = std::env::temp_dir().join(format!("doze-detect-empty-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let r = scan(&d, &go_kinds());
    let _ = std::fs::remove_dir_all(&d);
    assert!(r.is_empty());
}
