//! The reader against records the Go code wrote (tools/doze-oracle): recorded in tests/fixtures, so this runs
//! anywhere, and written now when Go and a doze checkout beside doze-rs are here.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use doze_projects::{StackKey, decode, list, slug};
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Go's `projects.List`, one project per line with its folder under home as `dir`, against ours.
fn agree(home: &Path, go: &str) {
    let ours = list(home);
    let go: Vec<_> = go.lines().collect();
    assert_eq!(ours.len(), go.len());
    for (p, line) in ours.iter().zip(go) {
        let want = decode(line).unwrap();
        let dir: serde_json::Value = serde_json::from_str(line).unwrap();
        let rel = dir["dir"].as_str().unwrap().trim_start_matches('/');
        assert_eq!(p.dir(), home.join(rel), "{line}");
        assert_eq!(
            (
                &p.schema,
                &p.config,
                &p.name,
                &p.last_up,
                &p.last_down,
                &p.last_set
            ),
            (
                &want.schema,
                &want.config,
                &want.name,
                &want.last_up,
                &want.last_down,
                &want.last_set
            ),
            "{line}"
        );
    }
}

#[test]
fn lists_the_recorded_go_projects_as_go_does() {
    let home = fixtures().join("home");
    agree(
        &home,
        &std::fs::read_to_string(fixtures().join("go-list.jsonl")).unwrap(),
    );
    let all = list(&home);
    let names: Vec<_> = all.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["shop", "", "harbour"],
        "the broken record is skipped"
    );
    // The newer doze's field survived Go's update, and its schema stayed its own.
    let harbour = &all[2];
    assert_eq!(harbour.schema, 2);
    assert_eq!(harbour.extra["tabs"].get(), r#"["home",{"pinned":true}]"#);
    assert_eq!(all[1].last_down.unwrap().offset().whole_minutes(), 330);
}

#[test]
fn every_slug_is_the_one_go_gives() {
    if !Path::new("/code").is_absolute() {
        eprintln!("skipped: the recorded configs are Unix paths");
        return;
    }
    let configs = [
        "/code/shop/doze.hcl",
        "/Users/me/My Shop!/doze.hcl",
        "/a/b/../c/./doze.hcl",
        "/x/__/doze.hcl",
        "/doze.hcl",
        "/code/Ünïcode app/doze.hcl",
        "/code/trailing/",
        "",
    ];
    let go = std::fs::read_to_string(fixtures().join("slugs.txt")).unwrap();
    let go: Vec<_> = go.lines().collect();
    assert_eq!(go.len(), configs.len());
    for (config, want) in configs.iter().zip(go) {
        let key = StackKey::for_config(Path::new("/h"), Path::new(config));
        assert_eq!(key.dir(), Path::new(want), "{config:?}");
        assert_eq!(format!("/h/projects/{}", slug(Path::new(config))), want);
    }
}

fn oracle() -> Option<(PathBuf, PathBuf)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doze = root.join("../doze");
    let go = Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    (go && doze.join("internal/projects/projects.go").exists())
        .then(|| (root.join("tools/doze-oracle/run.sh"), doze))
}

#[test]
fn lists_projects_go_records_now() {
    let Some((script, doze)) = oracle() else {
        eprintln!("skipped: needs Go and ../doze beside doze-rs");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let out = Command::new("sh")
        .arg(&script)
        .arg(&doze)
        .arg("projects")
        .arg(home.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    agree(home.path(), &String::from_utf8(out.stdout).unwrap());
}
