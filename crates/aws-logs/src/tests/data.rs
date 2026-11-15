//! `doze-aws data` on CloudWatch Logs: a group's size is its events in every stream; emptying keeps the group, its settings and
//! its streams; an older-than prune keeps the newer events; a group with no retention is pointed at; the file shrinks once done.

use std::sync::Arc;

use aws_wire::storage::{Emptied, Prune, Resource, StorageError};
use doze_runtime::ManualClock;
use serde_json::{Value, json};

use super::{NOW, World, temp};
use crate::core::Core;
use crate::storage::{DataAsk, DataReply};

const DAY: i64 = 86_400_000;
const GROUP: &str = "/aws/lambda/orders";

fn open(dir: &std::path::Path, clock: &Arc<ManualClock>) -> World {
    World::with(
        Core::open_in(dir, clock.clone(), None).unwrap(),
        clock.clone(),
    )
}

fn emptied(w: &mut World, ask: DataAsk) -> Result<Emptied, StorageError> {
    match w.core.data("us-east-1", ask) {
        DataReply::Emptied(done) => done,
        other => panic!("{other:?}"),
    }
}

fn empty(w: &mut World, prune: Prune) -> Result<Emptied, StorageError> {
    emptied(
        w,
        DataAsk::Empty {
            name: GROUP.into(),
            prune,
        },
    )
}

fn usage(w: &mut World) -> Resource {
    let DataReply::Usage(Ok((resources, _))) = w.core.data("us-east-1", DataAsk::Usage) else {
        panic!()
    };
    resources.into_iter().find(|r| r.name == GROUP).unwrap()
}

fn fill(w: &mut World, stream: &str, at: i64, count: usize) {
    let body = "x".repeat(8 << 10);
    let messages: Vec<&str> = (0..count).map(|_| body.as_str()).collect();
    w.put(GROUP, stream, at, &messages);
}

fn streams(w: &mut World) -> Vec<Value> {
    w.ok("DescribeLogStreams", &json!({"logGroupName": GROUP}))["logStreams"]
        .as_array()
        .unwrap()
        .clone()
}

fn events(w: &mut World, stream: &str) -> usize {
    w.ok(
        "GetLogEvents",
        &json!({"logGroupName": GROUP, "logStreamName": stream, "startFromHead": true}),
    )["events"]
        .as_array()
        .unwrap()
        .len()
}

/// A group as Lambda leaves it (no retention), with a metric filter and tags, and events in two streams: 200 ten and nine days
/// old, 50 an hour old.
fn lambda_group(w: &mut World) {
    w.group(GROUP);
    w.ok(
        "TagLogGroup",
        &json!({"logGroupName": GROUP, "tags": {"team": "a"}}),
    );
    w.ok(
        "PutMetricFilter",
        &json!({"logGroupName": GROUP, "filterName": "errors", "filterPattern": "ERROR",
            "metricTransformations": [{"metricName": "Errors", "metricNamespace": "App", "metricValue": "1"}]}),
    );
    w.stream(GROUP, "a");
    w.stream(GROUP, "b");
    fill(w, "a", NOW - 10 * DAY, 100);
    fill(w, "a", NOW - 9 * DAY, 100);
    fill(w, "b", NOW - DAY / 24, 50);
}

#[test]
fn a_group_is_measured_pruned_emptied_and_its_space_given_back() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(NOW));
    let mut w = open(dir.path(), &clock);
    lambda_group(&mut w);

    let r = usage(&mut w);
    assert_eq!(r.items, 250);
    assert!(r.bytes > 250 * (8 << 10), "{r:?}");
    assert_eq!(r.notes.len(), 1, "{r:?}");
    // The local bound (7 days by default) removes old events only when the group is next written or read: the note says both.
    assert!(
        r.notes[0].text.starts_with(
            "no retention set: kept forever on AWS; doze removes events older than 7 days"
        ),
        "{r:?}"
    );
    w.core.set(crate::Settings {
        retention: None,
        max_events: 0,
    });
    let note = usage(&mut w).notes[0].clone();
    assert_eq!(note.text, "no retention set: kept forever");
    assert!(
        note.bytes > 200 * (8 << 10) && note.bytes < r.bytes,
        "{note:?}"
    );
    let older = note.prune.unwrap();
    assert_eq!(older.before_ms, Some(NOW - 7 * DAY));

    let dry = Prune {
        dry_run: true,
        ..older.clone()
    };
    let would = empty(&mut w, dry).unwrap();
    assert_eq!(
        (would.items, would.bytes, would.freed),
        (200, note.bytes, 0)
    );
    let done = empty(&mut w, older).unwrap();
    assert_eq!(done.items, 200);
    assert!(done.freed > 1 << 20, "the file shrank: {done:?}");
    assert_eq!((events(&mut w, "a"), events(&mut w, "b")), (0, 50));
    let a = streams(&mut w)[0].clone();
    assert_eq!(a["logStreamName"], "a");
    assert!(a.get("firstEventTimestamp").is_none(), "{a}");

    // The stream takes events again, and an empty of the whole group keeps both streams.
    fill(&mut w, "a", NOW, 1);
    assert_eq!(empty(&mut w, Prune::default()).unwrap().items, 51);
    assert_eq!(usage(&mut w).items, 0);
    assert!(
        usage(&mut w).notes.is_empty(),
        "an empty group is not pointed at"
    );
    drop(w);
    kept_after_a_restart(open(dir.path(), &clock));
}

/// A restart loads exactly what the live core holds: the group, its tags, its metric filter and its two streams, with no events.
fn kept_after_a_restart(mut w: World) {
    assert_eq!(streams(&mut w).len(), 2);
    assert_eq!((events(&mut w, "a"), events(&mut w, "b")), (0, 0));
    let tags = w.ok("ListTagsLogGroup", &json!({"logGroupName": GROUP}));
    assert_eq!(tags["tags"], json!({"team": "a"}));
    let filters = w.ok("DescribeMetricFilters", &json!({"logGroupName": GROUP}));
    assert_eq!(filters["metricFilters"].as_array().unwrap().len(), 1);
    fill(&mut w, "b", NOW, 1);
    assert_eq!(usage(&mut w).items, 1);
    // A retention removes the reason for the note.
    w.ok(
        "PutRetentionPolicy",
        &json!({"logGroupName": GROUP, "retentionInDays": 7}),
    );
    assert!(usage(&mut w).notes.is_empty());

    let prefix = Prune {
        prefix: Some("a".into()),
        ..Prune::default()
    };
    assert!(matches!(
        empty(&mut w, prefix),
        Err(StorageError::Unsupported(_))
    ));
    let nope = emptied(
        &mut w,
        DataAsk::Empty {
            name: "/nope".into(),
            prune: Prune::default(),
        },
    );
    assert_eq!(nope, Err(StorageError::NotFound));
}

#[test]
fn a_reset_removes_every_group_and_survives_a_restart() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(NOW));
    let mut w = open(dir.path(), &clock);
    lambda_group(&mut w);
    w.ok(
        "PutQueryDefinition",
        &json!({"name": "q", "queryString": "fields @message"}),
    );
    assert_eq!(
        emptied(&mut w, DataAsk::Reset { dry_run: true })
            .unwrap()
            .items,
        250
    );
    let done = emptied(&mut w, DataAsk::Reset { dry_run: false }).unwrap();
    assert_eq!(done.items, 250);
    assert!(done.freed > 0, "{done:?}");
    drop(w);
    let mut w = open(dir.path(), &clock);
    assert_eq!(
        w.ok("DescribeLogGroups", &json!({}))["logGroups"],
        json!([])
    );
    assert_eq!(
        w.ok("DescribeQueryDefinitions", &json!({}))["queryDefinitions"],
        json!([])
    );
    // The name is free at once, with nothing of the old group in it.
    w.group(GROUP);
    w.stream(GROUP, "a");
    assert_eq!(events(&mut w, "a"), 0);
}
