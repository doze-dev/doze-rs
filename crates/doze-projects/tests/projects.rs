//! Project records, stack keys and discovery: Go's own tests (internal/projects/projects_test.go, the reading half),
//! the corners of Go's decoder a record can hit, and the stale-pid check.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use doze_projects::{FILE_NAME, StackKey, daemon_pid, decode, list, load, running};
use std::path::Path;

fn record(dir: &Path, text: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(FILE_NAME), text).unwrap();
}

// Go: TestSaveLoadAndList.
#[test]
fn loads_and_lists_most_recent_first() {
    let home = tempfile::tempdir().unwrap();
    let shop = home.path().join("projects/shop-1");
    let blog = home.path().join("projects/blog-2");
    assert!(load(&shop).unwrap().is_none(), "an unrecorded project");
    record(
        &shop,
        r#"{"schema":1,"config":"/code/shop/doze.hcl","name":"shop","last_up":"2026-11-13T10:00:00Z","last_set":["api"]}"#,
    );
    record(
        &blog,
        r#"{"schema":1,"config":"/code/blog/doze.hcl","last_up":"2026-11-13T09:00:00Z"}"#,
    );
    let p = load(&shop).unwrap().unwrap();
    assert_eq!(p.config, "/code/shop/doze.hcl");
    assert_eq!(p.last_set, ["api"]);
    assert_eq!(p.dir(), shop);
    // A broken record is skipped, not fatal.
    record(&home.path().join("projects/bad-3"), "{");
    let names: Vec<_> = list(home.path()).into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["shop", ""]);
}

// Go: TestUnknownFieldsAreKept (the reading half).
#[test]
fn fields_a_newer_doze_wrote_are_kept() {
    let p =
        decode(r#"{"schema":2,"config":"/x/doze.hcl","tabs":["home"],"Config2":1e400}"#).unwrap();
    assert_eq!(p.schema, 2);
    assert_eq!(p.extra["tabs"].get(), r#"["home"]"#);
    assert_eq!(p.extra["Config2"].get(), "1e400");
    assert!(!p.extra.contains_key("config"));
}

// Go: TestABrokenRecordIsReplaced (the reading half: it does not load).
#[test]
fn a_broken_record_does_not_load() {
    let dir = tempfile::tempdir().unwrap();
    record(dir.path(), "");
    let err = load(dir.path()).unwrap_err();
    assert!(err.to_string().contains(FILE_NAME), "{err}");
}

#[test]
fn records_decode_as_go_decodes_them() {
    // Keys match case-insensitively; a key that is not exactly a known one is also kept aside, as in Go.
    let p = decode(r#"{"Config":"/a","NAME":"n","last_set":["x",null]}"#).unwrap();
    assert_eq!((p.config.as_str(), p.name.as_str()), ("/a", "n"));
    assert_eq!(p.last_set, ["x", ""]);
    assert!(p.extra.contains_key("Config"));
    // null empties a list and leaves the rest; a null document is an empty record.
    assert!(
        decode(r#"{"last_set":["x"],"last_set":null}"#)
            .unwrap()
            .last_set
            .is_empty()
    );
    assert_eq!(decode("null").unwrap().schema, 0);
    // Go's zero time is "never".
    assert_eq!(
        decode(r#"{"last_up":"0001-01-01T00:00:00Z"}"#)
            .unwrap()
            .last_up,
        None
    );
    for bad in [
        r#"{"schema":"1"}"#,
        r#"{"schema":1.5}"#,
        r#"{"last_up":5}"#,
        r#"{"last_set":"api"}"#,
        "[]",
        "{",
    ] {
        assert!(decode(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_down_after_the_up_is_the_last_use() {
    let home = tempfile::tempdir().unwrap();
    record(
        &home.path().join("projects/a"),
        r#"{"name":"a","last_up":"2026-11-13T08:00:00Z","last_down":"2026-11-13T12:00:00+05:30"}"#,
    );
    record(
        &home.path().join("projects/b"),
        r#"{"name":"b","last_up":"2026-11-13T07:00:00Z"}"#,
    );
    record(&home.path().join("projects/c"), r#"{"name":"c"}"#);
    let names: Vec<_> = list(home.path()).into_iter().map(|p| p.name).collect();
    // a's down is 06:30Z, before its up: its up (08:00Z) is its last use.
    assert_eq!(names, ["a", "b", "c"]);
    assert!(list(&home.path().join("none")).is_empty());
}

#[test]
fn a_stack_key_names_its_files() {
    let key = StackKey::new("/h/projects/shop-1a2b3c4d");
    assert_eq!(key.slug(), "shop-1a2b3c4d");
    assert_eq!(StackKey::from_socket(&key.socket()), Some(key.clone()));
    assert!(key.socket().ends_with("run/doze.admin.sock"));
    assert!(key.pid_file().ends_with("run/doze.pid"));
    assert!(key.journal().ends_with("run/journal.jsonl"));
    assert!(key.log_file().ends_with("run/doze.log"));
    assert!(key.record().ends_with(FILE_NAME));
}

fn stack(home: &Path, slug: &str, pid: Option<&str>) -> StackKey {
    let key = StackKey::new(home.join("projects").join(slug));
    std::fs::create_dir_all(key.run_dir()).unwrap();
    std::fs::write(key.socket(), "").unwrap(); // a socket's place on disk is all discovery looks at
    if let Some(pid) = pid {
        std::fs::write(key.pid_file(), pid).unwrap();
    }
    key
}

/// A pid no process has: past any system's pid range, yet a valid pid.
const GONE: &str = "2147483646";

#[test]
fn discovery_skips_a_socket_whose_daemon_is_gone() {
    let home = tempfile::tempdir().unwrap();
    let me = std::process::id().to_string();
    let live = stack(home.path(), "live-1", Some(&format!("{me}\nextra\n")));
    let fixture = stack(home.path(), "fixture-2", None);
    stack(home.path(), "stale-3", Some(GONE));
    record(
        &home.path().join("projects/stopped-4"),
        r#"{"name":"stopped"}"#,
    );
    let found: Vec<_> = running(home.path())
        .into_iter()
        .map(|r| (r.key, r.pid))
        .collect();
    assert_eq!(found, [(fixture, None), (live, Some(std::process::id()))]);
}

#[test]
fn a_pid_file_says_what_go_reads_from_it() {
    let home = tempfile::tempdir().unwrap();
    let key = stack(home.path(), "s", None);
    assert_eq!(daemon_pid(&key), None, "no pid file");
    for (text, want) in [
        ("0", None),
        ("-4", None),
        ("abc", None),
        ("", None),
        (" 1 \n", Some(1)),
    ] {
        std::fs::write(key.pid_file(), text).unwrap();
        assert_eq!(daemon_pid(&key).map(|p| p.pid), want, "{text:?}");
    }
    std::fs::write(key.pid_file(), GONE).unwrap();
    assert!(!daemon_pid(&key).unwrap().alive);
    std::fs::write(key.pid_file(), "99999999999").unwrap();
    assert!(
        !daemon_pid(&key).unwrap().alive,
        "a pid past u32 names no process"
    );
}
