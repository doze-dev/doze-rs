use super::{compose_images, image_base, make_targets};
use doze_module_proto::msg::hints::{Detect, ModuleHints};
use std::collections::BTreeMap;
use std::path::Path;

/// Two modules' declarations, as their manifests give them (the scan knows no engine of its own).
fn hints() -> Vec<ModuleHints> {
    let hint =
        |module: &str, label: &str, go: &[&str], npm: &[&str], images: &[&str]| ModuleHints {
            module: module.into(),
            label: label.into(),
            client: None,
            detect: Some(Detect {
                packages: BTreeMap::from([
                    ("go".into(), go.iter().map(|s| s.to_string()).collect()),
                    ("npm".into(), npm.iter().map(|s| s.to_string()).collect()),
                ]),
                images: images.iter().map(|s| s.to_string()).collect(),
                env: BTreeMap::new(),
            }),
        };
    vec![
        hint(
            "postgres",
            "db",
            &["github.com/jackc/pgx"],
            &["pg"],
            &["postgres"],
        ),
        hint("valkey", "cache", &[], &["ioredis"], &["valkey/valkey"]),
        ModuleHints {
            module: "quiet".into(),
            label: "quiet".into(),
            client: None,
            detect: None,
        },
    ]
}

fn detect(dir: &Path) -> super::Detected {
    super::detect(dir, &hints())
}

/// The generic default: with no module declaring anything, nothing is guessed as a module; a compose image is a service.
#[test]
fn without_declarations_nothing_is_a_module() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    std::fs::write(
        p.join("package.json"),
        r#"{"dependencies":{"ioredis":"5","pg":"8"}}"#,
    )
    .unwrap();
    std::fs::write(
        p.join("compose.yaml"),
        "services:\n  db:\n    image: postgres:17\n",
    )
    .unwrap();
    let d = super::detect(p, &[]);
    assert!(d.modules.is_empty());
    assert_eq!(
        d.services.first().map(|s| s.becomes.as_str()),
        Some("service \"db\"")
    );
}

#[test]
fn a_node_app_with_a_postgres_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    std::fs::write(
        p.join("package.json"),
        r#"{"name":"shopfront","scripts":{"dev":"vite"},"dependencies":{"ioredis":"5"}}"#,
    )
    .unwrap();
    std::fs::write(p.join("pnpm-lock.yaml"), "").unwrap();
    std::fs::write(p.join("docker-compose.yml"), "services:\n  db:\n    image: postgres:17\n  mail:\n    image: \"axllent/mailpit\"\n    ports: [\"8025:8025\"]\n").unwrap();
    std::fs::write(
        p.join("Makefile"),
        "migrate:\n\tgo run ./migrate\nX := 1\nbuild:\n",
    )
    .unwrap();
    let d = detect(p);
    assert_eq!(d.counts(), ["1 app", "2 modules", "1 service", "1 task"]);
    let lines: Vec<(String, String, String)> = d
        .items()
        .into_iter()
        .map(|f| (f.name, f.where_, f.becomes))
        .collect();
    assert_eq!(
        lines.first(),
        Some(&(
            "shopfront".into(),
            "pnpm run dev".into(),
            "app \"shopfront\"".into()
        ))
    );
    assert!(lines.contains(&(
        "db".into(),
        "docker-compose.yml has a postgres service".into(),
        "module postgres \"db\"".into()
    )));
    assert!(lines.contains(&(
        "cache".into(),
        "package.json depends on ioredis".into(),
        "module valkey \"cache\"".into()
    )));
    assert!(
        lines
            .iter()
            .any(|l| l.0 == "mail" && l.1.starts_with("image axllent/mailpit · "))
    );
    assert!(lines.contains(&(
        "migrate".into(),
        "make migrate".into(),
        "task \"migrate\"".into()
    )));
}

#[test]
fn go_mains_and_requirements() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    std::fs::write(
        p.join("go.mod"),
        "module x\n\nrequire (\n\tgithub.com/jackc/pgx/v5 v5.6.0\n)\n",
    )
    .unwrap();
    std::fs::create_dir_all(p.join("cmd/api")).unwrap();
    std::fs::write(p.join("cmd/api/main.go"), "package main\n").unwrap();
    let d = detect(p);
    assert_eq!(
        d.apps.first().map(|a| a.where_.as_str()),
        Some("go run ./cmd/api")
    );
    assert_eq!(d.modules.first().map(|m| m.name.as_str()), Some("db"));
    assert!(detect(&p.join("nope")).items().is_empty());
}

#[test]
fn compose_and_make_reading() {
    assert_eq!(
        compose_images(
            "version: 3\nservices:\n\n  b:\n    image: redis:7 # cache\n  a:\n    build: .\nvolumes:\n  x:\n    image: no\n"
        ),
        [("b".into(), "redis:7".into())]
    );
    assert_eq!(
        image_base("docker.io/library/postgres:17@sha256:x"),
        "postgres"
    );
    assert_eq!(
        image_base("localhost:5000/team/app:1"),
        "localhost:5000/team/app"
    );
    assert_eq!(
        make_targets("a: b\nB_c-d:\n\techo\n.PHONY: a\nx := 1\n"),
        ["a", "B_c-d"]
    );
}
