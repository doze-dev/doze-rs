//! Where doze-config words a refusal its own way (difference 13 in `differences.rs`): an argument only an app takes,
//! said where it goes, and a refusal without a position that carries its fix.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use doze_config::{Config, Error, Options, load};
use std::collections::BTreeMap;
use support::{Fixtures, OracleHost};

/// Loads `text` as a stack's one doze.hcl, in a folder of its own.
fn load_one(name: &str, text: &str) -> Result<Config, Error> {
    let root = std::env::temp_dir().join(format!("doze-config-{}-{name}", std::process::id()));
    let (dir, home) = (root.join("stack"), root.join("home"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(dir.join("doze.hcl"), text).unwrap();
    let host = OracleHost {
        home,
        cwd: dir,
        zone: None,
    };
    let opts = Options {
        host: &host,
        modules: &Fixtures,
        vars: BTreeMap::new(),
        version: "dev".into(),
    };
    let out = load("doze.hcl", &opts);
    let _ = std::fs::remove_dir_all(&root);
    out
}

fn summary(e: &Error) -> String {
    e.diags
        .iter()
        .next()
        .map_or_else(|| e.text.clone(), |d| d.summary.clone())
}

#[test]
fn domains_on_a_service_are_said_to_be_for_apps() {
    for arg in ["domains = [\"mail.acme.com\"]", "cert = \"c.pem\""] {
        let src =
            format!("service \"mailpit\" {{\n  run = \"mailpit\"\n  port = 1025\n  {arg}\n}}\n");
        let e = load_one("svc-domains", &src).unwrap_err();
        let name = arg.split(' ').next().unwrap();
        assert_eq!(
            summary(&e),
            format!(
                "service \"mailpit\": `{name}` is for app blocks; a service is reached by its doze name"
            )
        );
        let at = e.diags.iter().next().unwrap().subject.unwrap();
        assert_eq!(
            (at.start.line, at.start.column),
            (4, 3),
            "the caret is on the argument"
        );
    }
    let src = "task \"build\" {\n  run = \"make\"\n  domains = [\"b.acme.com\"]\n}\n";
    let e = load_one("task-domains", src).unwrap_err();
    assert_eq!(
        summary(&e),
        "task \"build\": `domains` is for app blocks; a task runs to an end and serves nothing"
    );
}

#[test]
fn domains_without_names_say_how_to_turn_them_on() {
    let src = "app \"web\" {\n  run = \"x\"\n  port = 8080\n  domains = [\"dev.acme.com\"]\n}\n";
    let e = load_one("names-off", src).unwrap_err();
    assert_eq!(
        e.text,
        "\"web\" declares domains, which need names on: add `names = true` to the defaults block"
    );
    assert_eq!(
        e.fix.as_deref(),
        Some("add `names = true` to the defaults block in doze.hcl, then: doze lint")
    );
}
