//! Resharding: a split, a merge and a new shard count close shards and open children; a closed shard is read to its end; the stream
//! says `UPDATING` for a moment. (The first scenario is the one recorded on AWS, 2026-11-09; the rest are worded from memory.)

use serde_json::{Value, json};

use super::World;

pub(super) const BASE_MS: u64 = 1_700_000_000_000;
pub(super) const MID: u128 = 1 << 127;

pub(super) fn shards(w: &mut World, filter: &Value) -> Vec<Value> {
    let mut req = json!({"StreamName": "s"});
    if !filter.is_null() {
        req["ShardFilter"] = filter.clone();
    }
    w.ok("ListShards", &req)["Shards"]
        .as_array()
        .unwrap()
        .clone()
}

pub(super) fn ids(list: &[Value]) -> Vec<String> {
    list.iter()
        .map(|s| {
            crate::model::shard_number(s["ShardId"].as_str().unwrap())
                .unwrap()
                .to_string()
        })
        .collect()
}

pub(super) fn status(w: &mut World) -> String {
    w.ok("DescribeStreamSummary", &json!({"StreamName": "s"}))["StreamDescriptionSummary"]
        ["StreamStatus"]
        .as_str()
        .unwrap()
        .to_owned()
}

pub(super) fn settle(w: &mut World) {
    w.clock.advance(1000);
}

pub(super) fn split(w: &mut World, shard: &str, at: u128) -> Value {
    w.ok(
        "SplitShard",
        &json!({"StreamName": "s", "ShardToSplit": shard, "NewStartingHashKey": at.to_string()}),
    )
}

pub(super) fn shard(n: u32) -> String {
    crate::model::shard_name(n)
}

pub(super) fn data_of(records: &Value) -> Vec<String> {
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
