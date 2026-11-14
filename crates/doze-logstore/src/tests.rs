#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

fn write(dir: &Path, inst: &str, name: &str, text: &str) {
    std::fs::create_dir_all(dir.join(inst)).unwrap();
    std::fs::write(dir.join(inst).join(name), text).unwrap();
}

#[test]
fn tails_the_newest_run_and_the_one_before() {
    let t = tempfile::tempdir().unwrap();
    write(
        t.path(),
        "api",
        "r000001.s000.log",
        "2026-10-10T09:05:00.000000Z o listening on :8080\n2026-10-10T09:05:10.000000Z e panic\n",
    );
    write(
        t.path(),
        "api",
        "r000002.s000.log",
        "2026-10-10T09:15:00.000000Z s:setup migrated\n",
    );
    write(
        t.path(),
        "api",
        "r000002.s001.log",
        "2026-10-10T09:15:01.000000Z o up\n2026-10-10T09:15:02.000000Z o half",
    );
    write(t.path(), "api", "notes.txt", "not a segment\n");
    let s = Store::open(t.path());
    let newest = s.tail("api", 0, 200).unwrap();
    assert_eq!(
        newest.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        ["migrated", "up"]
    );
    assert_eq!((newest[0].run, newest[0].stream.as_str()), (2, "s:setup"));
    let before = s.tail("api", -1, 1).unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(
        (before[0].text.as_str(), before[0].stamp.as_str()),
        ("panic", "2026-10-10T09:05:10.000000Z")
    );
    assert_eq!(s.tail("api", 1, 5).unwrap().len(), 2);
    assert!(matches!(
        s.tail("api", -2, 5),
        Err(Error::OnlyRuns { kept: 2, .. })
    ));
    assert!(matches!(
        s.tail("api", 7, 5),
        Err(Error::NoRun { run: 7, .. })
    ));
    assert!(s.tail("never", 0, 5).unwrap().is_empty());
}

#[test]
fn a_long_segment_is_read_from_its_end() {
    let t = tempfile::tempdir().unwrap();
    let mut text = String::new();
    for i in 0..20_000 {
        text.push_str(&format!("2026-10-10T09:15:00.000000Z o line {i}\n"));
    }
    write(t.path(), "db", "r000003.s000.log", &text);
    let got = Store::open(t.path()).tail("db", 0, 3).unwrap();
    assert_eq!(
        got.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        ["line 19997", "line 19998", "line 19999"]
    );
}

#[test]
fn names_and_lines_are_go_s_exactly() {
    assert_eq!(segment_name("r000012.s003.log"), Some((12, 3)));
    for bad in [
        "r12.s3.log",
        "r000012.s003.txt",
        "x000012.s003.log",
        "r0000a2.s003.log",
    ] {
        assert_eq!(segment_name(bad), None, "{bad}");
    }
    assert!(parse_line("a", 1, b"2026-10-10T09:15:00Z o short stamp").is_none());
    assert!(parse_line("a", 1, b"nospace").is_none());
    let l = parse_line("a", 1, b"2026-10-10T09:15:00.000001Z e").unwrap();
    assert_eq!((l.stream.as_str(), l.text.as_str()), ("e", ""));
}
