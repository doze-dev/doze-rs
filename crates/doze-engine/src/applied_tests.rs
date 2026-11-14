#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

fn obj(kind: &str, name: &str, hash: &str) -> Object {
    Object {
        kind: kind.into(),
        name: name.into(),
        hash: hash.into(),
    }
}

/// Go's `TestLoadSaveRoundTrip`, `TestLoadMissingReturnsEmpty`; `.doze/` gets its `.gitignore`; a task record
/// round-trips.
#[test]
fn load_save_round_trip() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join(".doze/state.json");
    assert_eq!(Applied::load(&path).unwrap(), Applied::default());
    let mut s = Applied::default();
    s.set(
        "app",
        vec![obj("role", "a", "1"), obj("database", "app", "2")],
    );
    s.outputs.insert("url".into(), "postgres://x".into());
    s.tasks.insert(
        "migrate".into(),
        TaskRecord {
            result: "done".into(),
            took_ms: 12,
            ..TaskRecord::default()
        },
    );
    s.save(&path).unwrap();
    let back = Applied::load(&path).unwrap();
    assert_eq!(back.instances, s.instances);
    assert_eq!(
        back.tasks["migrate"].ran_at, "0001-01-01T00:00:00Z",
        "Go's zero time, never \"\""
    );
    assert_eq!(
        std::fs::read_to_string(t.path().join(".doze/.gitignore")).unwrap(),
        "# Written by doze: this folder is this machine's state, so nothing in it is committed.\n*\n"
    );
    assert_eq!(path_for(&t.path().join("doze.hcl")), path);
}

/// What Go wrote (`json.MarshalIndent`, two spaces) reads, and what Rust writes has Go's shape.
#[test]
fn reads_and_writes_gos_json() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("state.json");
    let go = r#"{
  "version": 1,
  "instances": {
    "db": [
      {
        "kind": "role",
        "name": "app",
        "hash": "abc"
      }
    ]
  },
  "tasks": {
    "seed": {
      "result": "failed",
      "exit_code": 2,
      "took_ms": 40,
      "ran_at": "2026-11-13T09:30:00.123456789Z",
      "why": "first run",
      "inputs": ""
    }
  }
}"#;
    std::fs::write(&path, go).unwrap();
    let s = Applied::load(&path).unwrap();
    assert_eq!(s.objects("db"), [obj("role", "app", "abc")]);
    assert_eq!(s.tasks["seed"].exit_code, 2);
    s.save(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), go);
}

/// Go's `TestDiffCreateUpdateDelete`, `TestRemovedIsReverseOrder`, `TestSetEmptyClearsInstance`.
#[test]
fn diff_removed_and_set() {
    let prior = [obj("role", "a", "1"), obj("database", "x", "1")];
    let desired = [obj("role", "a", "2"), obj("schema", "s", "1")];
    let d: Vec<(Change, String)> = diff(&prior, &desired)
        .into_iter()
        .map(|(c, o)| (c, o.name))
        .collect();
    assert_eq!(
        d,
        [
            (Change::Update, "a".to_string()),
            (Change::Create, "s".to_string()),
            (Change::Delete, "x".to_string())
        ]
    );
    let prior = [
        obj("role", "a", "1"),
        obj("database", "x", "1"),
        obj("schema", "s", "1"),
    ];
    let gone: Vec<String> = removed(&prior, &[obj("role", "a", "1")])
        .into_iter()
        .map(|o| o.name)
        .collect();
    assert_eq!(gone, ["s", "x"]);
    let mut s = Applied::default();
    s.set("a", vec![obj("role", "r", "1")]);
    s.set("a", vec![]);
    assert!(!s.instances.contains_key("a"));
    assert_eq!(Change::Delete.symbol(), "-");
}

/// One sync at a time: a second lock is refused while the first is held, and taken once it is dropped.
#[test]
fn one_sync_at_a_time() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join(".doze/state.json");
    let held = lock(&path).unwrap();
    // A lock in this process on another handle: the system says it is held.
    let again = lock(&path);
    assert!(again.is_err_and(|e| e.to_string().contains("another doze sync")));
    drop(held);
    assert!(lock(&path).is_ok());
}
