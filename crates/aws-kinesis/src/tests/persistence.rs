//! State survives a stop: streams, shards, and the records on disk.

use std::sync::Arc;

use doze_runtime::ManualClock;
use serde_json::json;

use super::{World, temp};
use crate::core::Core;

fn open(dir: &std::path::Path, clock: &Arc<ManualClock>) -> World {
    World::with(
        Core::open_in(dir, clock.clone(), None).unwrap(),
        clock.clone(),
    )
}

#[test]
fn streams_and_records_survive_a_restart_and_an_old_iterator_still_reads() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let mut w = open(dir.path(), &clock);
    w.stream("s", 2);
    w.ok(
        "AddTagsToStream",
        &json!({"StreamName": "s", "Tags": {"a": "b"}}),
    );
    w.put("s", "a", "one");
    w.put("s", "a", "two");
    let it = w.iterator("s", "shardId-000000000000", "TRIM_HORIZON");
    drop(w);
    // The writer thread finishes what it has when the core is dropped (its database is closed by the drop).
    let mut w = open(dir.path(), &clock);
    let summary = w.ok("DescribeStreamSummary", &json!({"StreamName": "s"}));
    assert_eq!(summary["StreamDescriptionSummary"]["OpenShardCount"], 2);
    assert_eq!(
        w.ok("ListTagsForStream", &json!({"StreamName": "s"}))["Tags"],
        json!([{"Key": "a", "Value": "b"}])
    );
    let got = w.ok("GetRecords", &json!({"ShardIterator": it}));
    assert_eq!(got["Records"].as_array().unwrap().len(), 2);
    // A put after the restart takes a later sequence number than anything before it.
    let last = got["Records"][1]["SequenceNumber"]
        .as_str()
        .unwrap()
        .parse::<u128>()
        .unwrap();
    let again = w.put("s", "a", "three")["SequenceNumber"]
        .as_str()
        .unwrap()
        .parse::<u128>()
        .unwrap();
    assert!(again > last);
}

#[test]
fn a_deleted_stream_takes_its_records_with_it() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let mut w = open(dir.path(), &clock);
    w.stream("s", 1);
    w.put("s", "k", "x");
    w.ok("DeleteStream", &json!({"StreamName": "s"}));
    // A stream of the same name starts empty.
    w.stream("s", 1);
    let it = w.iterator("s", "shardId-000000000000", "TRIM_HORIZON");
    let got = w.ok("GetRecords", &json!({"ShardIterator": it}));
    assert!(got["Records"].as_array().unwrap().is_empty());
}

#[test]
fn a_put_writes_one_row_for_the_record_and_one_for_the_stream() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let mut w = open(dir.path(), &clock);
    w.stream("s", 1);
    w.put("s", "k", "x");
    // A read changes nothing on disk.
    let it = w.iterator("s", "shardId-000000000000", "TRIM_HORIZON");
    w.ok("GetRecords", &json!({"ShardIterator": it}));
    assert!(w.core.streams.stage().unwrap().is_empty());
    assert!(w.core.rows.stage().is_empty());
}
