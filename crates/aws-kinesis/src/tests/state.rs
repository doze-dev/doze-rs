//! What a reshard leaves behind: filtered listings, retention, a restart, and the account-level commitment.

use std::sync::Arc;

use doze_runtime::ManualClock;
use serde_json::{Value, json};

use super::reshard::{BASE_MS, MID, data_of, ids, settle, shard, shards, split};
use super::{World, temp};
use crate::core::Core;

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
