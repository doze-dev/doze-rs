//! The writer against records Go's `projects.Update` wrote (tools/doze-oracle `update`): each case starts from the
//! same record (`<case>.in`, or none) and makes the same change, and the bytes must be Go's. Recorded in
//! tests/fixtures/updates, so this runs anywhere; written again now when Go and a doze checkout beside doze-rs are here.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use doze_projects::{FILE_NAME, Project, load, update};
use std::path::{Path, PathBuf};
use std::process::Command;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

fn at() -> OffsetDateTime {
    let day = Date::from_calendar_date(2026, Month::November, 13).unwrap();
    PrimitiveDateTime::new(day, Time::from_hms_nano(9, 30, 0, 123_456_789).unwrap()).assume_utc()
}

/// The change each case of the Go oracle makes, by name.
fn change(case: &str) -> fn(&mut Project) {
    match case {
        "fresh" => |p| {
            p.config = "/code/shop/doze.hcl".into();
            p.name = "shop".into();
            p.last_up = Some(at());
            p.last_set = vec!["api".into(), "worker".into()];
        },
        "newer" => |p| p.last_down = Some(at()),
        "broken" => |p| p.config = "/c/doze.hcl".into(),
        "escapes" => |p| {
            p.config = "/c/a&b <x>/doze.hcl".into();
            p.name = "line\u{2028}sep\u{1}\u{8}\u{c}\t\"\\é".into();
        },
        "cleared" => |p| {
            p.last_set.clear();
            p.name.clear();
        },
        "null" => |p| p.name = "x".into(),
        "rawbytes" => |p| p.name = "n".into(),
        "fraction" => |p| {
            p.config = "/c".into();
            let west = UtcOffset::from_hms(-3, -30, 0).unwrap();
            let day = Date::from_calendar_date(2026, Month::January, 1).unwrap();
            let wall =
                PrimitiveDateTime::new(day, Time::from_hms_nano(0, 0, 0, 500_000_000).unwrap());
            p.last_up = Some(wall.assume_offset(west));
        },
        other => panic!("no such case {other}"),
    }
}

const CASES: [&str; 8] = [
    "fresh", "newer", "broken", "escapes", "cleared", "null", "fraction", "rawbytes",
];

/// Runs every case here, starting from Go's inputs in `go`, and compares each file with Go's.
fn agree(go: &Path) {
    let ours = tempfile::tempdir().unwrap();
    for case in CASES {
        let dir = ours.path().join(case);
        if let Ok(input) = std::fs::read(go.join(format!("{case}.in"))) {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(FILE_NAME), input).unwrap();
        }
        update(&dir, change(case)).unwrap();
        for file in [FILE_NAME, "project.json.bad"] {
            let want = std::fs::read(go.join(case).join(file)).ok();
            let got = std::fs::read(dir.join(file)).ok();
            assert_eq!(got, want, "{case}/{file}: bytes differ");
        }
        // What was written reads back as written.
        let back = load(&dir).unwrap().unwrap();
        assert_eq!(doze_projects::encode(&back).unwrap(), {
            let mut b = std::fs::read(dir.join(FILE_NAME)).unwrap();
            b.pop();
            b
        });
    }
}

#[test]
fn writes_what_go_wrote() {
    agree(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/updates"));
}

#[test]
fn the_record_is_private_and_nothing_is_left_beside_it() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("projects/x-1");
    update(&dir, change("fresh")).unwrap();
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, [FILE_NAME]);
    if let Some(mode) = doze_os::fs::mode(&dir.join(FILE_NAME)) {
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn a_record_that_cannot_be_set_aside_is_an_error_and_nothing_is_written() {
    let home = tempfile::tempdir().unwrap();
    // A folder where the record should be: unreadable as a file; set aside it is, so make the rename fail by
    // occupying `.bad` with a folder that is not empty.
    let dir = home.path().join("p");
    std::fs::create_dir_all(dir.join(FILE_NAME)).unwrap();
    std::fs::create_dir_all(dir.join("project.json.bad/keep")).unwrap();
    assert!(update(&dir, change("fresh")).is_err());
    assert!(dir.join(FILE_NAME).is_dir());
}

#[test]
fn writes_what_go_writes_now() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doze = root.join("../doze");
    let go = Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !go || !doze.join("internal/projects/projects.go").exists() {
        eprintln!("skipped: needs Go and ../doze beside doze-rs");
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let status = Command::new("sh")
        .arg(root.join("tools/doze-oracle/run.sh"))
        .arg(&doze)
        .arg("update")
        .arg(out.path())
        .status()
        .unwrap();
    assert!(status.success());
    agree(out.path());
}
