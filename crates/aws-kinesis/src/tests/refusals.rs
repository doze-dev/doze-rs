//! The refusals AWS gave when the scenarios were recorded (2026-11-09): the code and the words.

use serde_json::json;

use super::World;

#[test]
fn a_stream_that_is_not_there_is_not_found_in_aws_words() {
    let mut w = World::new();
    for (op, req) in [
        ("DescribeStreamSummary", json!({"StreamName": "absent"})),
        ("DeleteStream", json!({"StreamName": "absent"})),
        (
            "PutRecord",
            json!({"StreamName": "absent", "PartitionKey": "k", "Data": "eA=="}),
        ),
    ] {
        let (code, message) = w.err(op, &req);
        assert_eq!(code, "ResourceNotFoundException", "{op}");
        assert_eq!(
            message, "Stream absent under account 000000000000 not found.",
            "{op}"
        );
    }
}

#[test]
fn creating_twice_and_a_missing_shard() {
    let mut w = World::new();
    w.stream("s", 1);
    let (code, message) = w.err("CreateStream", &json!({"StreamName": "s", "ShardCount": 1}));
    assert_eq!(code, "ResourceInUseException");
    assert_eq!(
        message,
        "Stream s under account 000000000000 already exists."
    );
    let (code, message) = w.err(
        "GetShardIterator",
        &json!({"StreamName": "s", "ShardId": "shardId-000000000099", "ShardIteratorType": "LATEST"}),
    );
    assert_eq!(code, "ResourceNotFoundException");
    assert_eq!(
        message,
        "Shard shardId-000000000099 in stream s under account 000000000000 does not exist"
    );
}

#[test]
fn iterator_and_retention_refusals() {
    let mut w = World::new();
    w.stream("s", 1);
    let (code, message) = w.err(
        "GetShardIterator",
        &json!({"StreamName": "s", "ShardId": "shardId-000000000000", "ShardIteratorType": "AT_SEQUENCE_NUMBER"}),
    );
    assert_eq!(code, "InvalidArgumentException");
    assert_eq!(
        message,
        "Must either specify (1) AT_SEQUENCE_NUMBER or AFTER_SEQUENCE_NUMBER and StartingSequenceNumber or (2) TRIM_HORIZON or LATEST and no StartingSequenceNumber. Request specified AT_SEQUENCE_NUMBER and no StartingSequenceNumber."
    );
    let (code, message) = w.err("GetRecords", &json!({"ShardIterator": "not-an-iterator"}));
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("InvalidArgumentException", "Invalid ShardIterator.")
    );
    let (code, message) = w.err(
        "DecreaseStreamRetentionPeriod",
        &json!({"StreamName": "s", "RetentionPeriodHours": 12}),
    );
    assert_eq!(code, "InvalidArgumentException");
    assert_eq!(
        message,
        "Minimum allowed retention period is 24 hours. Requested retention period (12 hours) is too short."
    );
    // "Increase" to what it already is is accepted (recorded).
    w.ok(
        "IncreaseStreamRetentionPeriod",
        &json!({"StreamName": "s", "RetentionPeriodHours": 24}),
    );
}

#[test]
fn a_bad_explicit_hash_key_is_a_validation_error_worded_from_the_model() {
    let mut w = World::new();
    w.stream("s", 1);
    let (code, message) = w.err(
        "PutRecord",
        &json!({"StreamName": "s", "PartitionKey": "k", "Data": "eA==", "ExplicitHashKey": "not-a-number"}),
    );
    assert_eq!(code, "ValidationException");
    assert_eq!(
        message,
        "1 validation error detected: Value 'not-a-number' at 'explicitHashKey' failed to satisfy constraint: Member must satisfy regular expression pattern: ^(0|([1-9]\\d{0,38}))$"
    );
}

/// What a request asks the authorizer for is read from its decoded input: a batch on its stream (named or by ARN), a read on the stream
/// its iterator was handed out for.
#[test]
fn puts_and_reads_are_asked_for_on_their_stream() {
    let mut w = World::new();
    w.stream("s", 1);
    let arn = "arn:aws:kinesis:us-east-1:000000000000:stream/s";
    let needs = |w: &World, op: &str, body: serde_json::Value| {
        let call = w.h.call(w.h.op(op), &body).unwrap();
        let need = crate::authz::needs(&call).remove(0);
        (need.action, need.resource)
    };
    let batch = json!({"StreamName": "s", "Records": [{"PartitionKey": "k", "Data": "eA=="}]});
    assert_eq!(
        needs(&w, "PutRecords", batch),
        ("kinesis:PutRecords".to_owned(), arn.to_owned())
    );
    let by_arn = json!({"StreamARN": arn, "Records": [{"PartitionKey": "k", "Data": "eA=="}]});
    assert_eq!(needs(&w, "PutRecords", by_arn).1, arn);
    let iterator = w.iterator("s", "shardId-000000000000", "TRIM_HORIZON");
    assert_eq!(
        needs(&w, "GetRecords", json!({"ShardIterator": iterator})),
        ("kinesis:GetRecords".to_owned(), arn.to_owned())
    );
    assert_eq!(needs(&w, "ListStreams", json!({})).1, "*");
}
