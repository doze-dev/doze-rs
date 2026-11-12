//! `UpdateShardCount`: closing every open shard and tiling the hash space again. (Worded from memory; see `reshard.rs`.)

use serde_json::{Value, json};

use super::World;
use super::reshard::{data_of, ids, settle, shard, shards, status};

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
