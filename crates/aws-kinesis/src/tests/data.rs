//! `doze-aws data` on Kinesis: a stream's size is its records; emptying keeps the stream, its shards and their numbering; an
//! older-than prune keeps the newer records; a raised retention is pointed at; the file shrinks once it is done.

use std::sync::Arc;

use aws_wire::storage::{Emptied, Prune, Resource, StorageError};
use doze_runtime::ManualClock;
use serde_json::{Value, json};

use super::{World, temp};
use crate::core::Core;
use crate::storage::{DataAsk, DataReply};

const HOUR: i64 = 3_600_000;

/// Runs a data question on a runtime of one thread, as the server does (a compaction goes to its blocking pool).
fn block<T>(asked: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(asked)
}

fn open(dir: &std::path::Path, clock: &Arc<ManualClock>) -> World {
    World::with(
        Core::open_in(dir, clock.clone(), None).unwrap(),
        clock.clone(),
    )
}

fn emptied(w: &mut World, ask: DataAsk) -> Result<Emptied, StorageError> {
    match block(w.core.data("us-east-1", ask)) {
        DataReply::Emptied(done) => done,
        other => panic!("{other:?}"),
    }
}

fn empty(w: &mut World, name: &str, prune: Prune) -> Result<Emptied, StorageError> {
    emptied(
        w,
        DataAsk::Empty {
            name: name.into(),
            prune,
        },
    )
}

fn usage(w: &mut World, name: &str) -> Resource {
    let DataReply::Usage(Ok((resources, _))) = block(w.core.data("us-east-1", DataAsk::Usage))
    else {
        panic!()
    };
    resources.into_iter().find(|r| r.name == name).unwrap()
}

fn fill(w: &mut World, count: usize) -> u128 {
    let body = "x".repeat(8 << 10);
    let mut last = 0;
    for n in 0..count {
        let put = w.put("events", &format!("k{n}"), &body);
        last = seq(&put);
    }
    last
}

fn seq(v: &Value) -> u128 {
    crate::model::seq_parse(v["SequenceNumber"].as_str().unwrap()).unwrap()
}

#[test]
fn a_stream_is_measured_pruned_emptied_and_its_space_given_back() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let mut w = open(dir.path(), &clock);
    w.stream("events", 2);
    w.ok(
        "AddTagsToStream",
        &json!({"StreamName": "events", "Tags": {"team": "a"}}),
    );
    w.ok(
        "IncreaseStreamRetentionPeriod",
        &json!({"StreamName": "events", "RetentionPeriodHours": 48}),
    );
    fill(&mut w, 300);
    clock.advance(25 * HOUR);
    fill(&mut w, 5);

    let r = usage(&mut w, "events");
    assert_eq!(r.items, 305);
    assert!(r.bytes > 305 * (8 << 10), "{r:?}");
    assert_eq!(r.notes.len(), 1, "a raised retention is pointed at");
    let note = r.notes[0].clone();
    assert!(
        note.bytes > 300 * (8 << 10) && note.bytes < r.bytes,
        "{note:?}"
    );
    let older = note.prune.unwrap();

    // A dry run counts and removes nothing; the note's prune removes just the records past a day.
    let dry = Prune {
        dry_run: true,
        ..older.clone()
    };
    let would = empty(&mut w, "events", dry).unwrap();
    assert_eq!(
        (would.items, would.bytes, would.freed),
        (300, note.bytes, 0)
    );
    let done = empty(&mut w, "events", older).unwrap();
    assert_eq!(done.items, 300);
    assert!(done.freed > 1 << 20, "the file shrank: {done:?}");
    assert_eq!(usage(&mut w, "events").items, 5);

    // Emptied whole: an iterator taken before reads on from the next record put, which is numbered above every earlier one.
    let it = w.iterator("events", "shardId-000000000000", "TRIM_HORIZON");
    let last = fill(&mut w, 1);
    assert_eq!(empty(&mut w, "events", Prune::default()).unwrap().items, 6);
    assert_eq!(usage(&mut w, "events").items, 0);
    let read = w.ok("GetRecords", &json!({"ShardIterator": it}));
    assert_eq!(read["Records"], json!([]));
    let next = w.put("events", "a", "after");
    assert!(seq(&next) > last);
    drop(w);
    kept_after_a_restart(open(dir.path(), &clock), seq(&next));
}

/// A restart loads exactly what the live core holds: the stream, its shards, retention and tags, and the one record put after
/// emptying (numbered `next`).
fn kept_after_a_restart(mut w: World, next: u128) {
    let s = w.ok("DescribeStreamSummary", &json!({"StreamName": "events"}));
    assert_eq!(s["StreamDescriptionSummary"]["OpenShardCount"], 2);
    assert_eq!(s["StreamDescriptionSummary"]["RetentionPeriodHours"], 48);
    let tags = w.ok("ListTagsForStream", &json!({"StreamName": "events"}));
    assert_eq!(tags["Tags"], json!([{"Key": "team", "Value": "a"}]));
    assert_eq!(usage(&mut w, "events").items, 1);
    assert!(seq(&w.put("events", "a", "again")) > next);

    let prefix = Prune {
        prefix: Some("a".into()),
        ..Prune::default()
    };
    assert!(matches!(
        empty(&mut w, "events", prefix),
        Err(StorageError::Unsupported(_))
    ));
    assert_eq!(
        empty(&mut w, "nope", Prune::default()),
        Err(StorageError::NotFound)
    );
}

#[test]
fn a_reset_removes_every_stream_and_survives_a_restart() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let mut w = open(dir.path(), &clock);
    w.stream("events", 1);
    fill(&mut w, 50);
    assert_eq!(
        emptied(&mut w, DataAsk::Reset { dry_run: true })
            .unwrap()
            .items,
        50
    );
    let done = emptied(&mut w, DataAsk::Reset { dry_run: false }).unwrap();
    assert_eq!(done.items, 50);
    assert!(done.freed > 0, "{done:?}");
    drop(w);
    let mut w = open(dir.path(), &clock);
    assert_eq!(w.ok("ListStreams", &json!({}))["StreamNames"], json!([]));
    // The name is free at once.
    w.stream("events", 1);
    assert_eq!(usage(&mut w, "events").items, 0);
}
