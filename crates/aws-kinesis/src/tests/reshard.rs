//! Resharding: a split, a merge and a new shard count close shards and open children; a closed shard is read to its end; the stream
//! says `UPDATING` for a moment. (The first scenario is the one recorded on AWS, 2026-11-09; the rest are worded from memory.)

use std::sync::Arc;

use doze_runtime::ManualClock;
use serde_json::{Value, json};

use super::{World, temp};
use crate::core::Core;

const BASE_MS: u64 = 1_700_000_000_000;
const MID: u128 = 1 << 127;

fn shards(w: &mut World, filter: &Value) -> Vec<Value> {
    let mut req = json!({"StreamName": "s"});
    if !filter.is_null() {
        req["ShardFilter"] = filter.clone();
    }
    w.ok("ListShards", &req)["Shards"]
        .as_array()
        .unwrap()
        .clone()
}

fn ids(list: &[Value]) -> Vec<String> {
    list.iter()
        .map(|s| {
            crate::model::shard_number(s["ShardId"].as_str().unwrap())
                .unwrap()
                .to_string()
        })
        .collect()
}

fn status(w: &mut World) -> String {
    w.ok("DescribeStreamSummary", &json!({"StreamName": "s"}))["StreamDescriptionSummary"]
        ["StreamStatus"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn settle(w: &mut World) {
    w.clock.advance(1000);
}

fn split(w: &mut World, shard: &str, at: u128) -> Value {
    w.ok(
        "SplitShard",
        &json!({"StreamName": "s", "ShardToSplit": shard, "NewStartingHashKey": at.to_string()}),
    )
}

fn shard(n: u32) -> String {
    crate::model::shard_name(n)
}

fn data_of(records: &Value) -> Vec<String> {
    records
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            String::from_utf8(aws_core::base64::decode(r["Data"].as_str().unwrap()).unwrap())
                .unwrap()
        })
        .collect()
}

#[test]
fn a_split_closes_the_parent_and_opens_two_children_as_recorded() {
    let mut w = World::new();
    w.stream("s", 1);
    // Recorded: the split point is half the end, the answer is empty, and the parent is closed at once though it never held a record.
    let end = u128::MAX;
    let half = end / 2;
    assert_eq!(split(&mut w, &shard(0), half), json!({}));
    let list = shards(&mut w, &Value::Null);
    assert_eq!(ids(&list), ["0", "1", "2"]);
    assert!(list[0]["SequenceNumberRange"]["EndingSequenceNumber"].is_string());
    assert!(list[1]["SequenceNumberRange"]["EndingSequenceNumber"].is_null());
    assert_eq!(list[1]["ParentShardId"], shard(0));
    assert_eq!(list[2]["ParentShardId"], shard(0));
    assert!(list[1]["AdjacentParentShardId"].is_null());
    assert_eq!(list[1]["HashKeyRange"]["StartingHashKey"], "0");
    assert_eq!(
        list[1]["HashKeyRange"]["EndingHashKey"],
        (half - 1).to_string()
    );
    assert_eq!(list[2]["HashKeyRange"]["StartingHashKey"], half.to_string());
    assert_eq!(list[2]["HashKeyRange"]["EndingHashKey"], end.to_string());
    // Distinct starting numbers, each after the parent's end.
    let seq = |v: &Value, k: &str| -> u128 {
        v["SequenceNumberRange"][k]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    };
    let parent_end = seq(&list[0], "EndingSequenceNumber");
    assert!(seq(&list[1], "StartingSequenceNumber") > parent_end);
    assert!(seq(&list[2], "StartingSequenceNumber") > seq(&list[1], "StartingSequenceNumber"));
}

#[test]
fn the_stream_is_updating_for_a_moment_and_refuses_a_second_reshard_meanwhile() {
    let mut w = World::new();
    w.stream("s", 1);
    assert_eq!(status(&mut w), "ACTIVE");
    split(&mut w, &shard(0), MID);
    assert_eq!(status(&mut w), "UPDATING");
    let listed = w.ok("ListStreams", &json!({}));
    assert_eq!(listed["StreamSummaries"][0]["StreamStatus"], "UPDATING");
    let described = w.ok("DescribeStream", &json!({"StreamName": "s"}));
    assert_eq!(described["StreamDescription"]["StreamStatus"], "UPDATING");
    let (code, message) = w.err(
        "SplitShard",
        &json!({"StreamName": "s", "ShardToSplit": shard(1), "NewStartingHashKey": "5"}),
    );
    assert_eq!(code, "ResourceInUseException");
    assert_eq!(
        message,
        "Stream s under account 000000000000 not ACTIVE, instead in state UPDATING"
    );
    let (code, _) = w.err(
        "UpdateShardCount",
        &json!({"StreamName": "s", "TargetShardCount": 4, "ScalingType": "UNIFORM_SCALING"}),
    );
    assert_eq!(code, "ResourceInUseException");
    // Reading and writing go on while it moves.
    assert_eq!(w.put("s", "a", "x")["ShardId"], shard(1));
    settle(&mut w);
    assert_eq!(status(&mut w), "ACTIVE");
    split(&mut w, &shard(1), 5);
}

#[test]
fn a_closed_shard_is_read_to_its_end_and_then_names_its_children() {
    let mut w = World::new();
    w.stream("s", 1);
    w.clock.advance(10);
    w.put("s", "a", "one");
    w.clock.advance(10);
    w.put("s", "b", "two");
    let old = w.iterator("s", &shard(0), "TRIM_HORIZON");
    let latest = w.iterator("s", &shard(0), "LATEST");
    split(&mut w, &shard(0), MID);

    // A read that stops short of the end carries on; the one that reaches it ends the shard.
    let first = w.ok("GetRecords", &json!({"ShardIterator": old, "Limit": 1}));
    assert_eq!(data_of(&first["Records"]), ["one"]);
    assert!(first["NextShardIterator"].is_string());
    assert!(first["ChildShards"].is_null());
    let second = w.ok(
        "GetRecords",
        &json!({"ShardIterator": first["NextShardIterator"], "Limit": 1}),
    );
    assert_eq!(data_of(&second["Records"]), ["two"]);
    let rest = w.ok(
        "GetRecords",
        &json!({"ShardIterator": second["NextShardIterator"]}),
    );
    assert!(rest["Records"].as_array().unwrap().is_empty());
    assert!(rest["NextShardIterator"].is_null());
    assert_eq!(rest["MillisBehindLatest"], 0);
    assert_eq!(
        rest["ChildShards"],
        json!([
            {"ShardId": shard(1), "ParentShards": [shard(0)],
             "HashKeyRange": {"StartingHashKey": "0", "EndingHashKey": (MID - 1).to_string()}},
            {"ShardId": shard(2), "ParentShards": [shard(0)],
             "HashKeyRange": {"StartingHashKey": MID.to_string(), "EndingHashKey": u128::MAX.to_string()}},
        ])
    );

    // All at once: the records and the end together.
    let all = w.iterator("s", &shard(0), "TRIM_HORIZON");
    let got = w.ok("GetRecords", &json!({"ShardIterator": all}));
    assert_eq!(data_of(&got["Records"]), ["one", "two"]);
    assert!(got["NextShardIterator"].is_null());
    assert_eq!(got["ChildShards"].as_array().unwrap().len(), 2);

    // An iterator made before the split, at the tip, reads nothing more and ends too.
    let tip = w.ok("GetRecords", &json!({"ShardIterator": latest}));
    assert!(tip["Records"].as_array().unwrap().is_empty());
    assert!(tip["NextShardIterator"].is_null());
    assert_eq!(tip["ChildShards"].as_array().unwrap().len(), 2);
}

#[test]
fn records_after_a_split_go_to_the_child_that_holds_their_hash_and_follow_the_parent_in_order() {
    let mut w = World::new();
    w.stream("s", 1);
    w.clock.advance(10);
    let before = w.put("s", "a", "old");
    split(&mut w, &shard(0), MID);
    settle(&mut w);
    // The recorded routing of two even shards: "a" is in the lower half, "b" in the upper.
    let a = w.put("s", "a", "new-a");
    let b = w.put("s", "b", "new-b");
    assert_eq!(a["ShardId"], shard(1));
    assert_eq!(b["ShardId"], shard(2));
    let seq = |v: &Value| {
        v["SequenceNumber"]
            .as_str()
            .unwrap()
            .parse::<u128>()
            .unwrap()
    };
    assert!(seq(&a) > seq(&before));
    assert!(seq(&b) > seq(&before));
    let list = shards(&mut w, &Value::Null);
    for child in &list[1..] {
        let start: u128 = child["SequenceNumberRange"]["StartingSequenceNumber"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!(start > seq(&before));
    }
    assert!(
        seq(&a)
            >= list[1]["SequenceNumberRange"]["StartingSequenceNumber"]
                .as_str()
                .unwrap()
                .parse::<u128>()
                .unwrap()
    );
    let it = w.iterator("s", &shard(1), "TRIM_HORIZON");
    let got = w.ok("GetRecords", &json!({"ShardIterator": it}));
    assert_eq!(data_of(&got["Records"]), ["new-a"]);
    // An open shard keeps its iterator.
    assert!(got["NextShardIterator"].is_string());
    assert!(got["ChildShards"].is_null());
    // An explicit hash key lands where the ranges say.
    let by_hash = w.ok(
        "PutRecord",
        &json!({"StreamName": "s", "PartitionKey": "a", "ExplicitHashKey": MID.to_string(), "Data": "eA=="}),
    );
    assert_eq!(by_hash["ShardId"], shard(2));
}

#[test]
fn a_split_point_must_lie_inside_the_shard_and_an_unknown_or_closed_shard_is_refused() {
    let mut w = World::new();
    w.stream("s", 1);
    let (code, message) = w.err(
        "SplitShard",
        &json!({"StreamName": "s", "ShardToSplit": shard(9), "NewStartingHashKey": "5"}),
    );
    assert_eq!(code, "ResourceNotFoundException");
    assert_eq!(
        message,
        "Shard shardId-000000000009 in stream s under account 000000000000 does not exist"
    );
    // The start of the shard would leave an empty lower child.
    let (code, message) = w.err(
        "SplitShard",
        &json!({"StreamName": "s", "ShardToSplit": shard(0), "NewStartingHashKey": "0"}),
    );
    assert_eq!(code, "InvalidArgumentException");
    assert!(message.starts_with(
        "NewStartingHashKey 0 used in SplitShard() on shard shardId-000000000000 in stream s"
    ));
    split(&mut w, &shard(0), MID);
    settle(&mut w);
    // Past the end of a child.
    let (code, _) = w.err(
        "SplitShard",
        &json!({"StreamName": "s", "ShardToSplit": shard(1), "NewStartingHashKey": MID.to_string()}),
    );
    assert_eq!(code, "InvalidArgumentException");
    // The parent is closed.
    let (code, message) = w.err(
        "SplitShard",
        &json!({"StreamName": "s", "ShardToSplit": shard(0), "NewStartingHashKey": "5"}),
    );
    assert_eq!(code, "InvalidArgumentException");
    assert!(message.contains("is closed"));
    // The last key of a shard is a fair split point: the upper child holds that one key.
    split(&mut w, &shard(2), u128::MAX);
    settle(&mut w);
    let list = shards(&mut w, &json!({"Type": "AT_LATEST"}));
    assert_eq!(ids(&list), ["1", "3", "4"]);
    assert_eq!(
        list[2]["HashKeyRange"]["StartingHashKey"],
        u128::MAX.to_string()
    );
    assert_eq!(
        list[2]["HashKeyRange"]["EndingHashKey"],
        u128::MAX.to_string()
    );
}

#[test]
fn a_merge_joins_neighbours_into_a_child_that_names_both_parents() {
    let mut w = World::new();
    w.stream("s", 1);
    split(&mut w, &shard(0), MID);
    settle(&mut w);
    w.clock.advance(5);
    w.put("s", "a", "left");
    w.put("s", "b", "right");
    let left = w.iterator("s", &shard(1), "TRIM_HORIZON");
    let right = w.iterator("s", &shard(2), "TRIM_HORIZON");
    // The order of the two does not matter for adjacency; the shard to merge is the first parent.
    let out = w.ok(
        "MergeShards",
        &json!({"StreamName": "s", "ShardToMerge": shard(2), "AdjacentShardToMerge": shard(1)}),
    );
    assert_eq!(out, json!({}));
    assert_eq!(status(&mut w), "UPDATING");
    let list = shards(&mut w, &Value::Null);
    assert_eq!(ids(&list), ["0", "1", "2", "3"]);
    assert_eq!(list[3]["ParentShardId"], shard(2));
    assert_eq!(list[3]["AdjacentParentShardId"], shard(1));
    assert_eq!(list[3]["HashKeyRange"]["StartingHashKey"], "0");
    assert_eq!(
        list[3]["HashKeyRange"]["EndingHashKey"],
        u128::MAX.to_string()
    );
    assert!(list[1]["SequenceNumberRange"]["EndingSequenceNumber"].is_string());
    assert!(list[2]["SequenceNumberRange"]["EndingSequenceNumber"].is_string());
    // Each parent is read to its end, and the child it hands over to names both parents.
    for (it, data) in [(left, "left"), (right, "right")] {
        let got = w.ok("GetRecords", &json!({"ShardIterator": it}));
        assert_eq!(data_of(&got["Records"]), [data]);
        assert!(got["NextShardIterator"].is_null());
        assert_eq!(
            got["ChildShards"],
            json!([{"ShardId": shard(3), "ParentShards": [shard(2), shard(1)],
                    "HashKeyRange": {"StartingHashKey": "0", "EndingHashKey": u128::MAX.to_string()}}])
        );
    }
    settle(&mut w);
    assert_eq!(w.put("s", "a", "merged")["ShardId"], shard(3));
}

#[test]
fn a_merge_of_shards_that_are_not_neighbours_is_refused() {
    let mut w = World::new();
    w.stream("s", 3);
    let (code, message) = w.err(
        "MergeShards",
        &json!({"StreamName": "s", "ShardToMerge": shard(0), "AdjacentShardToMerge": shard(2)}),
    );
    assert_eq!(code, "InvalidArgumentException");
    assert_eq!(
        message,
        "Shards shardId-000000000000 and shardId-000000000002 in stream s under account 000000000000 are not an adjacent pair of shards eligible for merging."
    );
    let (code, _) = w.err(
        "MergeShards",
        &json!({"StreamName": "s", "ShardToMerge": shard(1), "AdjacentShardToMerge": shard(1)}),
    );
    assert_eq!(code, "InvalidArgumentException");
    let (code, _) = w.err(
        "MergeShards",
        &json!({"StreamName": "s", "ShardToMerge": shard(0), "AdjacentShardToMerge": shard(7)}),
    );
    assert_eq!(code, "ResourceNotFoundException");
    // Nothing was changed by the refusals.
    assert_eq!(shards(&mut w, &Value::Null).len(), 3);
    assert_eq!(status(&mut w), "ACTIVE");
    // A closed shard cannot be merged.
    w.ok(
        "MergeShards",
        &json!({"StreamName": "s", "ShardToMerge": shard(0), "AdjacentShardToMerge": shard(1)}),
    );
    settle(&mut w);
    let (code, message) = w.err(
        "MergeShards",
        &json!({"StreamName": "s", "ShardToMerge": shard(0), "AdjacentShardToMerge": shard(2)}),
    );
    assert_eq!(code, "InvalidArgumentException");
    assert!(message.contains("is closed"), "{message}");
}

fn scale(w: &mut World, to: i64) -> Result<Value, (String, String)> {
    let req = json!({"StreamName": "s", "TargetShardCount": to, "ScalingType": "UNIFORM_SCALING"});
    w.run("UpdateShardCount", &req).map_err(|e| {
        (
            e.shape
                .map_or_else(|| e.code.to_string(), |s| s.to_string()),
            e.message,
        )
    })
}

#[test]
fn a_new_shard_count_closes_every_open_shard_and_tiles_again() {
    let mut w = World::new();
    w.stream("s", 2);
    w.clock.advance(10);
    w.put("s", "a", "x");
    let out = scale(&mut w, 4).unwrap();
    assert_eq!(out["StreamName"], "s");
    assert_eq!(out["CurrentShardCount"], 2);
    assert_eq!(out["TargetShardCount"], 4);
    assert_eq!(
        out["StreamARN"],
        "arn:aws:kinesis:us-east-1:000000000000:stream/s"
    );
    assert_eq!(status(&mut w), "UPDATING");
    let list = shards(&mut w, &Value::Null);
    assert_eq!(ids(&list), ["0", "1", "2", "3", "4", "5"]);
    let open = shards(&mut w, &json!({"Type": "AT_LATEST"}));
    assert_eq!(ids(&open), ["2", "3", "4", "5"]);
    // Doubling splits each shard at its middle, so a child names one parent.
    for (child, parent) in [(2, 0), (3, 0), (4, 1), (5, 1)] {
        assert_eq!(list[child]["ParentShardId"], shard(parent), "child {child}");
        assert!(list[child]["AdjacentParentShardId"].is_null());
    }
    let mut next = 0u128;
    for child in &list[2..] {
        assert_eq!(child["HashKeyRange"]["StartingHashKey"], next.to_string());
        next = child["HashKeyRange"]["EndingHashKey"]
            .as_str()
            .unwrap()
            .parse::<u128>()
            .unwrap()
            .wrapping_add(1);
    }
    assert_eq!(next, 0, "the last shard ends at the top");
}

#[test]
fn halving_the_shard_count_merges_pairs_and_the_parents_hand_over_to_their_children() {
    let mut w = World::new();
    w.stream("s", 2);
    w.clock.advance(10);
    w.put("s", "a", "x");
    scale(&mut w, 4).unwrap();
    settle(&mut w);
    // Halving merges pairs: a child names both.
    let out = scale(&mut w, 2).unwrap();
    assert_eq!(out["CurrentShardCount"], 4);
    let list = shards(&mut w, &Value::Null);
    assert_eq!(list.len(), 8);
    assert_eq!(list[6]["ParentShardId"], shard(2));
    assert_eq!(list[6]["AdjacentParentShardId"], shard(3));
    assert_eq!(list[7]["ParentShardId"], shard(4));
    assert_eq!(list[7]["AdjacentParentShardId"], shard(5));
    let summary = w.ok("DescribeStreamSummary", &json!({"StreamName": "s"}));
    assert_eq!(summary["StreamDescriptionSummary"]["OpenShardCount"], 2);
    // The record on shard 0 is read to its end, and the children it hands over to are the two it was split into.
    let it = w.iterator("s", &shard(0), "TRIM_HORIZON");
    let got = w.ok("GetRecords", &json!({"ShardIterator": it}));
    assert_eq!(data_of(&got["Records"]), ["x"]);
    let kids: Vec<&str> = got["ChildShards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["ShardId"].as_str().unwrap())
        .collect();
    assert_eq!(kids, [shard(2), shard(3)]);
}

#[test]
fn an_uneven_new_count_names_the_shards_that_owned_the_ends_of_each_child() {
    let mut w = World::new();
    w.stream("s", 3);
    scale(&mut w, 2).unwrap();
    let list = shards(&mut w, &Value::Null);
    // Thirds to halves: the lower half spans shards 0 and 1, the upper half shards 1 and 2.
    assert_eq!(list[3]["ParentShardId"], shard(0));
    assert_eq!(list[3]["AdjacentParentShardId"], shard(1));
    assert_eq!(list[4]["ParentShardId"], shard(1));
    assert_eq!(list[4]["AdjacentParentShardId"], shard(2));
}

#[test]
fn the_shard_count_is_bounded_by_double_and_half_and_by_the_stream_mode() {
    let mut w = World::new();
    w.stream("s", 2);
    let (code, message) = scale(&mut w, 5).unwrap_err();
    assert_eq!(code, "LimitExceededException");
    assert!(message.contains("from 2 to 5"), "{message}");
    w.stream("big", 4);
    let (code, _) = w
        .run(
            "UpdateShardCount",
            &json!({"StreamName": "big", "TargetShardCount": 1, "ScalingType": "UNIFORM_SCALING"}),
        )
        .map(|_| (String::new(), String::new()))
        .unwrap_or_else(|e| (e.code.to_string(), e.message));
    assert_eq!(code, "LimitExceededException");
    // The count it has already is not a change.
    let same = scale(&mut w, 2).unwrap();
    assert_eq!(same["CurrentShardCount"], 2);
    assert_eq!(same["TargetShardCount"], 2);
    assert_eq!(status(&mut w), "ACTIVE");
    assert_eq!(shards(&mut w, &Value::Null).len(), 2);
    // An on-demand stream scales itself.
    w.ok(
        "CreateStream",
        &json!({"StreamName": "auto", "StreamModeDetails": {"StreamMode": "ON_DEMAND"}}),
    );
    let (code, _) = w
        .run(
            "UpdateShardCount",
            &json!({"StreamName": "auto", "TargetShardCount": 8, "ScalingType": "UNIFORM_SCALING"}),
        )
        .map(|_| (String::new(), String::new()))
        .unwrap_or_else(|e| (e.code.to_string(), e.message));
    assert_eq!(code, "ValidationException");
    // A stream that is not there.
    let (code, _) = w
        .run(
            "UpdateShardCount",
            &json!({"StreamName": "nope", "TargetShardCount": 8, "ScalingType": "UNIFORM_SCALING"}),
        )
        .map(|_| (String::new(), String::new()))
        .unwrap_or_else(|e| (e.code.to_string(), e.message));
    assert_eq!(code, "ResourceNotFoundException");
}

#[test]
fn the_shard_limit_holds_for_a_split() {
    let mut w = World::new();
    w.stream("full", 500);
    let (code, message) = w.err(
        "SplitShard",
        &json!({"StreamName": "full", "ShardToSplit": shard(0), "NewStartingHashKey": "5"}),
    );
    assert_eq!(code, "LimitExceededException");
    assert_eq!(
        message,
        "This request would exceed the shard limit for the account 000000000000 in us-east-1."
    );
    // Merging gives a shard back.
    w.ok(
        "MergeShards",
        &json!({"StreamName": "full", "ShardToMerge": shard(0), "AdjacentShardToMerge": shard(1)}),
    );
}

#[test]
fn list_shards_filters_by_type_time_and_position_and_pages_with_the_filter() {
    let mut w = World::new();
    w.stream("s", 1);
    w.clock.advance(10_000);
    split(&mut w, &shard(0), MID);
    settle(&mut w);
    w.clock.advance(5_000);
    // Shard 2 is split too, so that the open shards (1, 3, 4) are interleaved with a closed one (2).
    split(&mut w, &shard(2), u128::MAX - 5);
    settle(&mut w);
    let t = |extra_s: f64| BASE_MS as f64 / 1000.0 + extra_s;
    let cases: [(Value, &[&str]); 10] = [
        (Value::Null, &["0", "1", "2", "3", "4"]),
        (
            json!({"Type": "FROM_TRIM_HORIZON"}),
            &["0", "1", "2", "3", "4"],
        ),
        (json!({"Type": "AT_LATEST"}), &["1", "3", "4"]),
        (json!({"Type": "AT_TRIM_HORIZON"}), &["0"]),
        (
            json!({"Type": "AFTER_SHARD_ID", "ShardId": shard(1)}),
            &["2", "3", "4"],
        ),
        (json!({"Type": "AT_TIMESTAMP", "Timestamp": t(5.0)}), &["0"]),
        (
            json!({"Type": "AT_TIMESTAMP", "Timestamp": t(12.0)}),
            &["1", "2"],
        ),
        (
            json!({"Type": "AT_TIMESTAMP", "Timestamp": t(18.0)}),
            &["1", "3", "4"],
        ),
        (
            json!({"Type": "FROM_TIMESTAMP", "Timestamp": t(12.0)}),
            &["1", "2", "3", "4"],
        ),
        (
            json!({"Type": "FROM_TIMESTAMP", "Timestamp": t(1.0)}),
            &["0", "1", "2", "3", "4"],
        ),
    ];
    for (filter, expected) in cases {
        assert_eq!(ids(&shards(&mut w, &filter)), expected, "{filter}");
    }
    // A type that needs a number or a shard says so.
    let (code, _) = w.err(
        "ListShards",
        &json!({"StreamName": "s", "ShardFilter": {"Type": "AT_TIMESTAMP"}}),
    );
    assert_eq!(code, "InvalidArgumentException");
    let (code, _) = w.err(
        "ListShards",
        &json!({"StreamName": "s", "ShardFilter": {"Type": "AFTER_SHARD_ID"}}),
    );
    assert_eq!(code, "InvalidArgumentException");
    // The second page is cut by the filter the first was asked with.
    let page = w.ok(
        "ListShards",
        &json!({"StreamName": "s", "MaxResults": 1, "ShardFilter": {"Type": "AT_LATEST"}}),
    );
    assert_eq!(ids(page["Shards"].as_array().unwrap()), ["1"]);
    let page = w.ok(
        "ListShards",
        &json!({"NextToken": page["NextToken"], "MaxResults": 1}),
    );
    assert_eq!(ids(page["Shards"].as_array().unwrap()), ["3"]);
    let page = w.ok("ListShards", &json!({"NextToken": page["NextToken"]}));
    assert_eq!(ids(page["Shards"].as_array().unwrap()), ["4"]);
    assert!(page["NextToken"].is_null());
}

#[test]
fn a_closed_shard_is_forgotten_once_its_records_are_past_the_retention() {
    let mut w = World::new();
    w.stream("s", 1);
    w.clock.advance(10);
    w.put("s", "a", "old");
    split(&mut w, &shard(0), MID);
    settle(&mut w);
    assert_eq!(shards(&mut w, &Value::Null).len(), 3);
    w.clock.advance(25 * 3_600_000);
    w.put("s", "a", "new");
    let list = shards(&mut w, &Value::Null);
    assert_eq!(ids(&list), ["1", "2"]);
    // The children still say whose they were.
    assert_eq!(list[0]["ParentShardId"], shard(0));
    let (code, _) = w.err(
        "GetShardIterator",
        &json!({"StreamName": "s", "ShardId": shard(0), "ShardIteratorType": "TRIM_HORIZON"}),
    );
    assert_eq!(code, "ResourceNotFoundException");
}

#[test]
fn a_reshard_survives_a_restart() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let open = |clock: &Arc<ManualClock>| {
        World::with(
            Core::open_in(dir.path(), clock.clone(), None).unwrap(),
            clock.clone(),
        )
    };
    let mut w = open(&clock);
    w.stream("s", 1);
    w.clock.advance(10);
    w.put("s", "a", "one");
    let it = w.iterator("s", &shard(0), "TRIM_HORIZON");
    split(&mut w, &shard(0), MID);
    drop(w);
    let mut w = open(&clock);
    let list = shards(&mut w, &Value::Null);
    assert_eq!(ids(&list), ["0", "1", "2"]);
    assert!(list[0]["SequenceNumberRange"]["EndingSequenceNumber"].is_string());
    assert_eq!(list[2]["ParentShardId"], shard(0));
    let got = w.ok("GetRecords", &json!({"ShardIterator": it}));
    assert_eq!(data_of(&got["Records"]), ["one"]);
    assert!(got["NextShardIterator"].is_null());
    assert_eq!(got["ChildShards"].as_array().unwrap().len(), 2);
    // The next shard takes the next number, not one already used.
    settle(&mut w);
    split(&mut w, &shard(1), 7);
    assert_eq!(
        ids(&shards(&mut w, &Value::Null)),
        ["0", "1", "2", "3", "4"]
    );
}

#[test]
fn the_minimum_throughput_billing_commitment_moves_through_its_states() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let open = |clock: &Arc<ManualClock>| {
        World::with(
            Core::open_in(dir.path(), clock.clone(), None).unwrap(),
            clock.clone(),
        )
    };
    let set = |w: &mut World, status: &str| {
        w.ok(
            "UpdateAccountSettings",
            &json!({"MinimumThroughputBillingCommitment": {"Status": status}}),
        )["MinimumThroughputBillingCommitment"]
            .clone()
    };
    let mut w = open(&clock);
    let describe = |w: &mut World| {
        w.ok("DescribeAccountSettings", &json!({}))["MinimumThroughputBillingCommitment"].clone()
    };
    assert_eq!(describe(&mut w), json!({"Status": "DISABLED"}));
    assert_eq!(set(&mut w, "DISABLED"), json!({"Status": "DISABLED"}));
    let on = set(&mut w, "ENABLED");
    assert_eq!(on["Status"], "ENABLED");
    assert_eq!(on["StartedAt"], 1_700_000_000.0);
    assert_eq!(on["EarliestAllowedEndAt"], 1_700_000_000.0 + 86_400.0);
    assert!(on["EndedAt"].is_null());
    // Before the earliest end a disable is scheduled; enabling again cancels it.
    w.clock.advance(3_600_000);
    assert_eq!(
        set(&mut w, "DISABLED")["Status"],
        "ENABLED_UNTIL_EARLIEST_ALLOWED_END"
    );
    assert_eq!(set(&mut w, "ENABLED")["Status"], "ENABLED");
    assert_eq!(
        set(&mut w, "DISABLED")["Status"],
        "ENABLED_UNTIL_EARLIEST_ALLOWED_END"
    );
    drop(w);
    // It outlives a restart, and the scheduled end takes effect when its time comes.
    let mut w = open(&clock);
    assert_eq!(
        describe(&mut w)["Status"],
        "ENABLED_UNTIL_EARLIEST_ALLOWED_END"
    );
    w.clock.advance(24 * 3_600_000);
    let off = describe(&mut w);
    assert_eq!(off["Status"], "DISABLED");
    assert_eq!(off["EndedAt"], 1_700_000_000.0 + 86_400.0);
    // A later commitment starts afresh.
    let again = set(&mut w, "ENABLED");
    assert_eq!(again["Status"], "ENABLED");
    assert!(again["EndedAt"].is_null());
    assert_eq!(again["StartedAt"], 1_700_000_000.0 + 3_600.0 + 86_400.0);
}
