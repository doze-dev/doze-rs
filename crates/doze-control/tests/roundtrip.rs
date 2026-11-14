//! Every wire type against JSON Go wrote: each decodes and re-encodes to the same bytes.
//!
//! `testdata/go/types` holds each type of `internal/control/types.go` (and the journal's `Event` and `Cause`) at its
//! zero value and filled by reflection three ways (every field set, with escapes, negative and huge numbers, and
//! offsets), encoded by Go's own `json.Encoder`. `testdata/go/scripts` are the Go fixture hubs; `testdata/go/live` is a
//! session with a real Go daemon. Regenerate with `internal/control/fixture/capture_test.go` (`testdata/go/capture/capture_test.go` says how).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use doze_control::fixture::Entry;
use doze_control::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};

fn testdata(dir: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/go")
        .join(dir)
}

/// Decodes each line of `text` as `T` and checks it re-encodes to the same line. Returns how many lines.
fn same_bytes<T: DeserializeOwned + Serialize>(name: &str, text: &str) -> usize {
    let mut n = 0;
    for (i, line) in text.lines().enumerate().filter(|(_, l)| !l.is_empty()) {
        let v: T = serde_json::from_str(line).unwrap_or_else(|e| panic!("{name}:{}: {e}", i + 1));
        let again = codec::encode(&v).unwrap();
        assert_eq!(
            String::from_utf8(again).unwrap(),
            format!("{line}\n"),
            "{name}:{}",
            i + 1
        );
        n += 1;
    }
    n
}

fn check_type(file: &str, text: &str) {
    let ty = file.split('.').next().unwrap();
    match ty {
        "request" => same_bytes::<Request>(file, text),
        "hello" => same_bytes::<Hello>(file, text),
        "log_frame" => same_bytes::<LogFrame>(file, text),
        "event_frame" => same_bytes::<EventFrame>(file, text),
        "proc_view" => same_bytes::<ProcView>(file, text),
        "resource_view" => same_bytes::<ResourceView>(file, text),
        "action_view" => same_bytes::<ActionView>(file, text),
        "holder_view" => same_bytes::<HolderView>(file, text),
        "instance_view" => same_bytes::<InstanceView>(file, text),
        "response" => same_bytes::<Response>(file, text),
        "stat_view" => same_bytes::<StatView>(file, text),
        "frame" => same_bytes::<Frame>(file, text),
        "task_view" => same_bytes::<TaskView>(file, text),
        "console_spec" => same_bytes::<ConsoleSpec>(file, text),
        "console_tab" => same_bytes::<ConsoleTab>(file, text),
        "console_extension" => same_bytes::<ConsoleExtension>(file, text),
        "console_node" => same_bytes::<ConsoleNode>(file, text),
        "console_panel" => same_bytes::<ConsolePanel>(file, text),
        "console_column" => same_bytes::<ConsoleColumn>(file, text),
        "console_row" => same_bytes::<ConsoleRow>(file, text),
        "console_frame" => same_bytes::<ConsoleFrame>(file, text),
        "journal_event" => same_bytes::<Event>(file, text),
        "journal_cause" => same_bytes::<Cause>(file, text),
        other => panic!("{file}: no Rust type for Go type {other}"),
    };
}

#[test]
fn every_go_type_round_trips_byte_for_byte() {
    let mut files: Vec<_> = fs::read_dir(testdata("types"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let mut types = std::collections::BTreeSet::new();
    for path in &files {
        let file = path.file_name().unwrap().to_str().unwrap();
        check_type(file, &fs::read_to_string(path).unwrap());
        types.insert(file.split('.').next().unwrap().to_owned());
    }
    // 21 control types + the journal's Event and Cause, each at zero, full, alt and small.
    assert_eq!(types.len(), 23, "{types:?}");
    assert_eq!(files.len(), 23 * 4);
}

#[test]
fn every_frame_of_a_real_go_daemon_session_round_trips() {
    let mut frames = 0;
    for name in ["session.ndjson", "old-client.ndjson", "newer-client.ndjson"] {
        frames += same_bytes::<Frame>(
            name,
            &fs::read_to_string(testdata("live").join(name)).unwrap(),
        );
    }
    assert!(frames > 200, "{frames}");
}

#[test]
fn every_go_fixture_hub_script_round_trips() {
    let mut hubs = 0;
    for entry in fs::read_dir(testdata("scripts")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let text = fs::read_to_string(&path).unwrap();
        assert!(same_bytes::<Entry>(&name, &text) > 0);
        // And the script reads, and writes itself back as it was.
        let script = fixture::Script::parse(&name, &text).unwrap();
        assert_eq!(
            fixture::Script::parse(&name, &script.to_text()).unwrap(),
            script
        );
        hubs += 1;
    }
    assert_eq!(hubs, 11);
}

#[test]
fn values_read_back_as_go_meant_them() {
    let text = fs::read_to_string(testdata("types/instance_view.alt.json")).unwrap();
    let v: InstanceView = serde_json::from_str(&text).unwrap();
    assert_eq!(v.pid, -3);
    assert_eq!(v.cpu, 3.0);
    assert_eq!(v.idle_after, GoDuration(42));
    assert_eq!(v.healthy, Some(true));
    assert_eq!(v.started_at.as_str(), "2026-01-02T03:04:05.12+05:30");
    assert_eq!(v.started_at.unix_nanos(), Some(1_767_303_245_120_000_000));
    let req: Request =
        serde_json::from_str(&fs::read_to_string(testdata("types/request.alt.json")).unwrap())
            .unwrap();
    assert_eq!(req.since, JOURNAL_FROM_START);
    let row: ConsoleRow = serde_json::from_str(r#"{"cells":null}"#).unwrap();
    assert_eq!(row.cells, None);
    let zero: Frame =
        serde_json::from_str(&fs::read_to_string(testdata("types/frame.zero.json")).unwrap())
            .unwrap();
    assert!(zero.at.is_zero());
    assert_eq!(zero, Frame::default());
}

#[test]
fn requests_are_written_as_go_writes_them() {
    let sub = Subscription {
        topics: vec![Topic::Instances, Topic::Journal, Topic::Stats],
        since: JOURNAL_FROM_START,
        interval: Some(std::time::Duration::from_millis(500)),
        backlog: -1,
        ..Subscription::default()
    };
    let line = |r: Request| String::from_utf8(codec::encode(&r).unwrap()).unwrap();
    assert_eq!(
        line(Request {
            id: 3,
            ..Request::subscribe(&sub)
        }),
        "{\"id\":3,\"op\":\"subscribe\",\"topics\":[\"instances\",\"journal\",\"stats\"],\"since\":18446744073709551615,\"interval\":\"500ms\",\"backlog\":-1}\n"
    );
    assert_eq!(
        line(Request::hello("app")),
        "{\"hello\":{\"proto\":2,\"client\":\"app\"}}\n"
    );
    assert_eq!(line(Request::cancel(7)), "{\"id\":7,\"op\":\"cancel\"}\n");
    assert_eq!(
        line(Request::verb(Verb::Sync, "db")),
        "{\"op\":\"apply\",\"db\":\"db\"}\n"
    );
    let q = LogQuery {
        instance: "api".into(),
        run: -1,
        tail: 5,
        since: Timestamp::from_unix_nanos(1),
    };
    assert_eq!(
        line(Request::logs(&q)),
        "{\"op\":\"logs\",\"db\":\"api\",\"run\":-1,\"tail\":5,\"since_time\":\"1970-01-01T00:00:00.000000001Z\"}\n"
    );
}

#[test]
fn the_names_match_go() {
    // server.go dispatches 23 op names (with the aliases down/sleep, boot/wake, keepawake/pin); types.go has five
    // topics; daemon.go ten verbs.
    assert_eq!(Op::ALL.len(), 23);
    assert_eq!(Topic::ALL.len(), 5);
    assert_eq!(Verb::ALL.len(), 10);
    for op in Op::ALL {
        assert_eq!(Op::parse(op.as_str()), Some(*op));
    }
    for v in Verb::ALL {
        assert_eq!(v.op().as_str(), v.as_str());
    }
}
