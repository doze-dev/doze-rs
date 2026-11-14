//! The Rust ferret module against the Go one's recorded answers (tools/module-oracle). Every case that differs must
//! differ only by the module's listed improvements:
//! - nothing listens on TCP: Go's PostgreSQL port and FerretDB debug port were picked at random from 30000-40000 per
//!   plan; Rust's PostgreSQL has its socket only (the fixed 5432 names the socket file) and the debug handler is off;
//! - the extension hook is an argument list, not a shell line (PLAN.md D14);
//! - indexes are objects of their own (`db.coll$name`), so one removed from doze.hcl is dropped;
//! - `auth = true` and `user` blocks are refused at their line: with auth on, FerretDB 2.7 refuses doze's own setup
//!   (`createUser requires authentication`), so Go failed at boot; without it, Go ignored the users;
//! - a seed file that is not there is refused at its line (Go failed at converge): the published example's
//!   `./seed/products.json`, which the oracle's `/base` does not hold.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_test::{Harness, oracle};
use serde_json::{Value, json};

const CORPUS: &str = include_str!("../../../tools/module-oracle/corpus.jsonl");
const GOLDEN: &str = include_str!("../../../tools/module-oracle/golden.jsonl");

fn case(id: &str) -> String {
    CORPUS
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|c| c["id"] == id)
        .unwrap()["hcl"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Go's random ports in a string, as Rust's plan has them: the PostgreSQL port is 5432, the debug address is empty.
fn fixed_ports(s: &str) -> String {
    let mut out = s.to_string();
    for (i, _) in s.match_indices(['3']) {
        let Some(port) = s
            .get(i..i + 5)
            .filter(|p| p.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        let before = s.get(..i).unwrap_or_default();
        if before.ends_with("-p ") || before.ends_with("port=") {
            out = out.replace(
                &format!("{}{port}", &before[before.len() - 3..]),
                &format!("{}5432", &before[before.len() - 3..]),
            );
        }
    }
    out
}

fn normalise(go: &mut Value) {
    for p in go["plan"].as_array_mut().unwrap() {
        if p["name"] == "postgres" {
            let args = p["args"].as_array_mut().unwrap();
            args[5] = json!("5432");
            p["ready"] = json!(fixed_ports(p["ready"].as_str().unwrap()));
            // The hook is an argument list (PLAN.md D14), so the SQL is one argument, not shell-quoted text.
            p["hooks"][0] = json!(fixed_ports(p["hooks"][0].as_str().unwrap()).replace('"', ""));
        } else {
            let env = p["env_added"].as_object_mut().unwrap();
            env.insert("FERRETDB_DEBUG_ADDR".into(), json!(""));
            let url = env["FERRETDB_POSTGRESQL_URL"].as_str().unwrap().to_string();
            env.insert("FERRETDB_POSTGRESQL_URL".into(), json!(fixed_ports(&url)));
        }
    }
}

/// The index objects Rust adds after each collection that declares indexes.
fn with_index_objects(go: &mut Value, rust: &Value) {
    let Some(objects) = go["objects"].as_array() else {
        return;
    };
    let mut out = Vec::new();
    for o in objects {
        out.push(o.clone());
        if o["kind"] == "collection" {
            let prefix = format!("{}$", o["name"].as_str().unwrap());
            out.extend(
                rust["objects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| {
                        r["kind"] == "index" && r["name"].as_str().unwrap().starts_with(&prefix)
                    })
                    .cloned(),
            );
        }
    }
    go["objects"] = Value::Array(out);
}

#[tokio::test(flavor = "current_thread")]
async fn answers_as_the_go_module_did_except_for_the_listed_improvements() {
    let m = Harness {
        program: env!("CARGO_BIN_EXE_doze-module-ferret").into(),
        engine: "ferret".into(),
        bin_dir: "/tc/bin".into(),
        full: "9.9.9".into(),
    }
    .module()
    .await
    .unwrap();
    let diffs = oracle::compare(&m, "ferret", CORPUS, GOLDEN, &[]).await;
    assert!(!diffs.is_empty(), "the improvements must show");
    for d in &diffs {
        let hcl = case(&d.id);
        let auth = hcl.contains("user \"") || hcl.contains("auth = true");
        let absent_seed = hcl.contains("seed = \"./seed/");
        if auth || absent_seed {
            assert_eq!(d.go["refused"], false, "{}", d.id);
            assert_eq!(d.rust["refused"], true, "{} must be refused:\n{d:#?}", d.id);
            continue;
        }
        assert_eq!(
            d.go["refused"], d.rust["refused"],
            "{} is refused by one side only:\n{d:#?}",
            d.id
        );
        let mut want = d.go.clone();
        normalise(&mut want);
        with_index_objects(&mut want, &d.rust);
        assert_eq!(
            want, d.rust,
            "{} differs beyond the listed improvements",
            d.id
        );
    }
}

#[test]
fn go_ports_are_found_where_they_stand() {
    assert_eq!(fixed_ports("x -p 37617 -d"), "x -p 5432 -d");
    assert_eq!(fixed_ports("a?host=/s&port=36257"), "a?host=/s&port=5432");
    assert_eq!(fixed_ports("-D /tmp/d/31234"), "-D /tmp/d/31234");
}

#[tokio::test(flavor = "current_thread")]
async fn auth_and_users_are_refused_at_their_line() {
    let m = Harness {
        program: env!("CARGO_BIN_EXE_doze-module-ferret").into(),
        engine: "ferret".into(),
        bin_dir: "/tc/bin".into(),
        full: "9.9.9".into(),
    }
    .module()
    .await
    .unwrap();
    let hcl = "ferret \"a\" {\n  version = \"2.7\"\n  auth = true\n  user \"app\" {\n    password = \"x\"\n  }\n}\n";
    let parsed = doze_module_test::hcl::parse_block(hcl, &m.manifest.schema).unwrap();
    let inst = doze_module_proto::msg::instance::Instance {
        name: "a".into(),
        engine: "ferret".into(),
        version: "2.7".into(),
        data_dir: "/tmp/dzo/d/a".into(),
        socket_dir: "/tmp/dzo/s/a".into(),
        spec: parsed.spec,
        host: String::new(),
        listeners: Default::default(),
        deps: vec![],
        toolchain: None,
        dir: String::new(),
    };
    let e = m.decode(&inst).await.unwrap_err();
    let paths: Vec<String> = e
        .diagnostics
        .iter()
        .map(|d| format!("{:?}", d.path))
        .collect();
    assert_eq!(
        paths,
        [r#"[Key("auth")]"#, r#"[Key("user"), Index(0)]"#],
        "{e:?}"
    );
    assert!(
        e.diagnostics[0]
            .message
            .contains("createUser requires authentication"),
        "{e:?}"
    );
}
