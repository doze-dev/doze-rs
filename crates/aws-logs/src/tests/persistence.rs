//! State survives a stop.

use std::sync::Arc;

use doze_runtime::ManualClock;
use serde_json::json;

use super::{NOW, World, temp};
use crate::core::Core;

fn open(dir: &std::path::Path, clock: &Arc<ManualClock>) -> World {
    World::with(
        Core::open_in(dir, clock.clone(), None).unwrap(),
        clock.clone(),
    )
}

#[test]
fn groups_streams_events_and_filters_survive_a_restart() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(NOW));
    let mut w = open(dir.path(), &clock);
    w.group("/g");
    w.stream("/g", "app");
    w.put("/g", "app", NOW, &["one", "two"]);
    w.ok(
        "TagLogGroup",
        &json!({"logGroupName": "/g", "tags": {"a": "b"}}),
    );
    w.ok(
        "PutQueryDefinition",
        &json!({"name": "q", "queryString": "fields @message"}),
    );
    drop(w);
    let mut w = open(dir.path(), &clock);
    let got = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true}),
    );
    assert_eq!(got["events"].as_array().unwrap().len(), 2);
    assert_eq!(
        w.ok("ListTagsLogGroup", &json!({"logGroupName": "/g"}))["tags"],
        json!({"a": "b"})
    );
    assert_eq!(
        w.ok("DescribeQueryDefinitions", &json!({}))["queryDefinitions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // A write after the restart is ordered after what was there.
    w.put("/g", "app", NOW, &["three"]);
    let got = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true}),
    );
    let seen: Vec<&str> = got["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["message"].as_str().unwrap())
        .collect();
    // Events sort by their time, then by the order they came in.
    assert_eq!(seen, ["one", "three", "two"]);
}

#[test]
fn deleting_a_group_takes_its_events_with_it() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(NOW));
    let mut w = open(dir.path(), &clock);
    w.group("/g");
    w.stream("/g", "app");
    w.put("/g", "app", NOW, &["x"]);
    w.ok("DeleteLogGroup", &json!({"logGroupName": "/g"}));
    w.group("/g");
    w.stream("/g", "app");
    let got = w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true}),
    );
    assert!(got["events"].as_array().unwrap().is_empty());
}

#[test]
fn a_read_changes_nothing_on_disk() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(NOW));
    let mut w = open(dir.path(), &clock);
    w.group("/g");
    w.stream("/g", "app");
    w.put("/g", "app", NOW, &["x"]);
    w.ok(
        "GetLogEvents",
        &json!({"logGroupName": "/g", "logStreamName": "app", "startFromHead": true}),
    );
    w.ok("FilterLogEvents", &json!({"logGroupName": "/g"}));
    assert!(w.core.groups.stage().unwrap().is_empty());
    assert!(w.core.streams.stage().unwrap().is_empty());
    assert!(w.core.events.stage().is_empty());
}

#[test]
fn the_event_cap_holds_on_disk_and_across_a_restart() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(NOW));
    let mut w = open(dir.path(), &clock);
    w.core.max_events = 10;
    w.group("/g");
    w.stream("/g", "app");
    for i in 0..5 {
        w.put("/g", "app", NOW + i * 10, &["a", "b", "c"]);
    }
    drop(w);
    let mut w = open(dir.path(), &clock);
    w.core.max_events = 10;
    w.put("/g", "app", NOW + 100, &["d"]);
    let all = w.ok("FilterLogEvents", &json!({"logGroupName": "/g"}));
    let n = all["events"].as_array().unwrap().len();
    // Only what is on disk is removed, so a write leaves at most the cap plus the batch it wrote, and never less than nine.
    assert!((9..=11).contains(&n), "{n} events held");
    assert_eq!(
        all["events"].as_array().unwrap().last().unwrap()["message"],
        "d"
    );
}
