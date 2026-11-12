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
