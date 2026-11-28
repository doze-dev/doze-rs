use serde_json::json;

use super::World;

fn data_of(records: &serde_json::Value) -> Vec<String> {
    records
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let bytes = aws_core::base64::decode(r["Data"].as_str().unwrap()).unwrap();
            String::from_utf8(bytes).unwrap()
        })
        .collect()
}

#[test]
fn a_stream_is_made_described_listed_and_deleted() {
    let mut w = World::new();
    w.stream("orders", 2);
    let summary = w.ok("DescribeStreamSummary", &json!({"StreamName": "orders"}));
    let d = &summary["StreamDescriptionSummary"];
    // The members the AWS recording has (test_stream_lifecycle::summary).
    assert_eq!(d["OpenShardCount"], 2);
    assert_eq!(d["RetentionPeriodHours"], 24);
    assert_eq!(d["StreamStatus"], "ACTIVE");
    assert_eq!(d["StreamModeDetails"]["StreamMode"], "PROVISIONED");
    assert_eq!(d["EncryptionType"], "NONE");
    assert_eq!(d["EnhancedMonitoring"], json!([{"ShardLevelMetrics": []}]));
    assert_eq!(
        d["StreamARN"],
        "arn:aws:kinesis:us-east-1:000000000000:stream/orders"
    );
    let shards = w.ok("ListShards", &json!({"StreamName": "orders"}));
    assert_eq!(shards["Shards"].as_array().unwrap().len(), 2);
    assert_eq!(shards["Shards"][0]["ShardId"], "shardId-000000000000");
    assert_eq!(
        shards["Shards"][0]["HashKeyRange"]["EndingHashKey"],
        "170141183460469231731687303715884105727"
    );
    assert_eq!(
        shards["Shards"][1]["HashKeyRange"]["StartingHashKey"],
        "170141183460469231731687303715884105728"
    );
    let listed = w.ok("ListStreams", &json!({}));
    assert_eq!(listed["StreamNames"], json!(["orders"]));
    assert_eq!(listed["HasMoreStreams"], false);
    w.ok("DeleteStream", &json!({"StreamName": "orders"}));
    assert_eq!(w.ok("ListStreams", &json!({}))["StreamNames"], json!([]));
}

#[test]
fn records_are_read_back_in_order_and_an_iterator_continues_where_it_stopped() {
    let mut w = World::new();
    w.stream("s", 1);
    for text in ["one", "two", "three"] {
        w.clock.advance(1);
        w.put("s", "k", text);
    }
    let it = w.iterator("s", "shardId-000000000000", "TRIM_HORIZON");
    let first = w.ok("GetRecords", &json!({"ShardIterator": it, "Limit": 2}));
    assert_eq!(data_of(&first["Records"]), ["one", "two"]);
    let next = first["NextShardIterator"].as_str().unwrap();
    let rest = w.ok("GetRecords", &json!({"ShardIterator": next}));
    assert_eq!(data_of(&rest["Records"]), ["three"]);
    let again = w.ok(
        "GetRecords",
        &json!({"ShardIterator": rest["NextShardIterator"]}),
    );
    assert!(again["Records"].as_array().unwrap().is_empty());
    assert_eq!(again["MillisBehindLatest"], 0);
}

#[test]
fn every_iterator_type_starts_where_it_says() {
    let mut w = World::new();
    w.stream("s", 1);
    let mut seqs = Vec::new();
    for text in ["a", "b", "c"] {
        w.clock.advance(1000);
        seqs.push(
            w.put("s", "k", text)["SequenceNumber"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    let read = |w: &mut World, req: serde_json::Value| {
        let it = w.ok("GetShardIterator", &req)["ShardIterator"].clone();
        data_of(&w.ok("GetRecords", &json!({"ShardIterator": it}))["Records"])
    };
    let base = json!({"StreamName": "s", "ShardId": "shardId-000000000000"});
    let with = |kind: &str, extra: serde_json::Value| {
        let mut v = base.clone();
        v["ShardIteratorType"] = json!(kind);
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    };
    assert_eq!(
        read(&mut w, with("TRIM_HORIZON", json!({}))),
        ["a", "b", "c"]
    );
    assert!(read(&mut w, with("LATEST", json!({}))).is_empty());
    assert_eq!(
        read(
            &mut w,
            with(
                "AT_SEQUENCE_NUMBER",
                json!({"StartingSequenceNumber": seqs[1]})
            )
        ),
        ["b", "c"]
    );
    assert_eq!(
        read(
            &mut w,
            with(
                "AFTER_SEQUENCE_NUMBER",
                json!({"StartingSequenceNumber": seqs[1]})
            )
        ),
        ["c"]
    );
    // The third record arrived at 1_700_000_003 s.
    assert_eq!(
        read(
            &mut w,
            with("AT_TIMESTAMP", json!({"Timestamp": 1_700_000_002.5}))
        ),
        ["c"]
    );
    // A record put after a LATEST iterator was made is read through it.
    let latest = w.iterator("s", "shardId-000000000000", "LATEST");
    w.clock.advance(5);
    w.put("s", "k", "d");
    let got = w.ok("GetRecords", &json!({"ShardIterator": latest}));
    assert_eq!(data_of(&got["Records"]), ["d"]);
}

#[test]
fn partition_keys_are_routed_by_their_md5_and_an_explicit_hash_key_wins() {
    let mut w = World::new();
    w.stream("s", 2);
    // Recorded on AWS (test_partition_keys_route_across_shards).
    for (key, shard) in [
        ("a", "shardId-000000000000"),
        ("b", "shardId-000000000001"),
        ("c", "shardId-000000000000"),
        ("d", "shardId-000000000001"),
        ("user-1", "shardId-000000000001"),
        ("user-2", "shardId-000000000000"),
    ] {
        assert_eq!(w.put("s", key, "x")["ShardId"], shard, "{key}");
    }
    let hashed = w.ok(
        "PutRecord",
        &json!({"StreamName": "s", "PartitionKey": "b", "ExplicitHashKey": "0", "Data": "eA=="}),
    );
    assert_eq!(hashed["ShardId"], "shardId-000000000000");
}

#[test]
fn put_records_reports_each_record_and_how_many_failed() {
    let mut w = World::new();
    w.stream("s", 1);
    let out = w.ok(
        "PutRecords",
        &json!({"StreamName": "s", "Records": [
            {"PartitionKey": "k", "Data": "b25l"},
            {"PartitionKey": "k", "Data": "dHdv"},
        ]}),
    );
    assert_eq!(out["FailedRecordCount"], 0);
    let seqs: Vec<&str> = out["Records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["SequenceNumber"].as_str().unwrap())
        .collect();
    assert!(crate::model::seq_parse(seqs[0]).unwrap() < crate::model::seq_parse(seqs[1]).unwrap());
}

#[test]
fn records_older_than_the_retention_are_removed_when_the_stream_is_next_used() {
    let mut w = World::new();
    w.stream("s", 1);
    w.put("s", "k", "old");
    w.clock.advance(25 * 3_600_000);
    w.put("s", "k", "new");
    let it = w.iterator("s", "shardId-000000000000", "TRIM_HORIZON");
    let got = w.ok("GetRecords", &json!({"ShardIterator": it}));
    assert_eq!(data_of(&got["Records"]), ["new"]);
    // A longer retention keeps what the shorter one would have dropped.
    w.ok(
        "IncreaseStreamRetentionPeriod",
        &json!({"StreamName": "s", "RetentionPeriodHours": 48}),
    );
    w.clock.advance(23 * 3_600_000);
    w.put("s", "k", "newer");
    let it = w.iterator("s", "shardId-000000000000", "TRIM_HORIZON");
    let got = w.ok("GetRecords", &json!({"ShardIterator": it}));
    assert_eq!(data_of(&got["Records"]), ["new", "newer"]);
}

#[test]
fn an_expired_iterator_is_refused() {
    let mut w = World::new();
    w.stream("s", 1);
    let it = w.iterator("s", "shardId-000000000000", "TRIM_HORIZON");
    w.clock.advance(301_000);
    let (code, _) = w.err("GetRecords", &json!({"ShardIterator": it}));
    assert_eq!(code, "ExpiredIteratorException");
}

#[test]
fn tags_encryption_monitoring_policy_and_consumers_are_stored_and_returned() {
    let mut w = World::new();
    w.stream("s", 1);
    w.ok(
        "AddTagsToStream",
        &json!({"StreamName": "s", "Tags": {"env": "dev", "team": "a"}}),
    );
    let tags = w.ok("ListTagsForStream", &json!({"StreamName": "s"}));
    assert_eq!(
        tags["Tags"],
        json!([{"Key": "env", "Value": "dev"}, {"Key": "team", "Value": "a"}])
    );
    assert_eq!(tags["HasMoreTags"], false);
    w.ok(
        "RemoveTagsFromStream",
        &json!({"StreamName": "s", "TagKeys": ["team"]}),
    );
    let arn = "arn:aws:kinesis:us-east-1:000000000000:stream/s";
    assert_eq!(
        w.ok("ListTagsForResource", &json!({"ResourceARN": arn}))["Tags"],
        json!([{"Key": "env", "Value": "dev"}])
    );
    w.ok(
        "StartStreamEncryption",
        &json!({"StreamName": "s", "EncryptionType": "KMS", "KeyId": "alias/aws/kinesis"}),
    );
    let d = w.ok("DescribeStream", &json!({"StreamName": "s"}));
    assert_eq!(d["StreamDescription"]["EncryptionType"], "KMS");
    assert_eq!(d["StreamDescription"]["KeyId"], "alias/aws/kinesis");
    let on = w.ok(
        "EnableEnhancedMonitoring",
        &json!({"StreamName": "s", "ShardLevelMetrics": ["ALL"]}),
    );
    assert_eq!(on["DesiredShardLevelMetrics"].as_array().unwrap().len(), 7);
    w.ok(
        "PutResourcePolicy",
        &json!({"ResourceARN": arn, "Policy": "{}"}),
    );
    assert_eq!(
        w.ok("GetResourcePolicy", &json!({"ResourceARN": arn}))["Policy"],
        "{}"
    );

    let made = w.ok(
        "RegisterStreamConsumer",
        &json!({"StreamARN": arn, "ConsumerName": "c1"}),
    );
    assert_eq!(made["Consumer"]["ConsumerStatus"], "ACTIVE");
    let summary = w.ok("DescribeStreamSummary", &json!({"StreamName": "s"}));
    assert_eq!(summary["StreamDescriptionSummary"]["ConsumerCount"], 1);
    let listed = w.ok("ListStreamConsumers", &json!({"StreamARN": arn}));
    assert_eq!(listed["Consumers"].as_array().unwrap().len(), 1);
    let (code, _) = w.err("DeleteStream", &json!({"StreamName": "s"}));
    assert_eq!(code, "ResourceInUseException");
    w.ok(
        "DeleteStream",
        &json!({"StreamName": "s", "EnforceConsumerDeletion": true}),
    );
}

#[test]
fn an_append_is_flagged_for_whoever_waits_for_records_of_that_stream() {
    let mut w = World::new();
    w.stream("s", 1);
    w.stream("t", 1);
    assert!(w.core.appended.is_empty());
    w.put("s", "k", "x");
    assert_eq!(w.core.appended.keys().collect::<Vec<_>>(), ["s"]);
    assert_eq!(w.core.appended["s"].len(), 1, "and the shard that took it");
}

#[test]
fn a_sequence_number_is_56_digits_that_sort_as_text_and_read_back() {
    // The model bounds a sequence number to 129 digits; AWS gives 56 (49, then 54 more). Checked in models/kinesis.json
    // (SequenceNumber: pattern ^(0|([1-9]\d{0,128}))$), so any width up to that is legal and 56 is what clients are used to.
    let mut w = World::new();
    w.stream("s", 1);
    let mut seqs = Vec::new();
    for text in ["a", "b", "c"] {
        w.clock.advance(1000);
        seqs.push(
            w.put("s", "k", text)["SequenceNumber"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    for s in &seqs {
        assert_eq!(s.len(), 56, "{s}");
        assert!(
            s.starts_with("49") && s.bytes().all(|b| b.is_ascii_digit()),
            "{s}"
        );
    }
    assert!(seqs[0] < seqs[1] && seqs[1] < seqs[2], "{seqs:?}");
    // The record shows the same number, and the shard's range is as wide.
    let it = w.ok(
        "GetShardIterator",
        &json!({"StreamName": "s", "ShardId": "shardId-000000000000", "ShardIteratorType": "TRIM_HORIZON"}),
    )["ShardIterator"]
        .clone();
    let got = w.ok("GetRecords", &json!({"ShardIterator": it}));
    assert_eq!(got["Records"][1]["SequenceNumber"], seqs[1].as_str());
    let shards = w.ok("ListShards", &json!({"StreamName": "s"}));
    let first = shards["Shards"][0]["SequenceNumberRange"]["StartingSequenceNumber"]
        .as_str()
        .unwrap();
    assert_eq!(first.len(), 56);
    assert!(first < seqs[0].as_str());
    // A number written down before the width changed (digits only, a plain number) still reads; text that is not a number does not.
    let n = crate::model::seq_parse(&seqs[1]).unwrap();
    assert_eq!(crate::model::seq_parse(&n.to_string()), Some(n));
    assert_eq!(crate::model::seq_parse("49x"), None);
    assert_eq!(crate::model::seq_parse(""), None::<u128>);
}
