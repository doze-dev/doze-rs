//! `doze-aws data` on DynamoDB: a table's size is its items, index entries and stream records; emptying keeps the table, its
//! indexes, time to live, stream and tags, and the stream's numbering; the file shrinks once it is done.

use aws_wire::storage::{Emptied, Prune, StorageError};
use serde_json::json;

use super::World;
use crate::keys::Space;
use crate::storage::{DataAsk, DataReply};

fn data(w: &mut World, ask: DataAsk) -> DataReply {
    w.core.as_mut().unwrap().data("us-east-1", ask)
}

fn emptied(reply: DataReply) -> Result<Emptied, StorageError> {
    match reply {
        DataReply::Emptied(done) => done,
        other => panic!("{other:?}"),
    }
}

fn empty(w: &mut World, name: &str, prune: Prune) -> Result<Emptied, StorageError> {
    emptied(data(
        w,
        DataAsk::Empty {
            name: name.into(),
            prune,
        },
    ))
}

fn orders(w: &mut World) {
    w.ok(
        "CreateTable",
        json!({"TableName": "orders", "BillingMode": "PAY_PER_REQUEST", "Tags": [{"Key": "team", "Value": "a"}],
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "owner", "AttributeType": "S"}],
            "GlobalSecondaryIndexes": [{"IndexName": "by-owner", "KeySchema": [{"AttributeName": "owner", "KeyType": "HASH"}], "Projection": {"ProjectionType": "ALL"}}],
            "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_IMAGE"}}),
    );
    w.ok(
        "UpdateTimeToLive",
        json!({"TableName": "orders", "TimeToLiveSpecification": {"Enabled": true, "AttributeName": "exp"}}),
    );
}

fn fill(w: &mut World, table: &str, from: usize, count: usize) {
    let body = "x".repeat(4 << 10);
    for n in from..from + count {
        w.put(
            table,
            json!({"pk": {"S": format!("k{n}")}, "owner": {"S": "ada"}, "exp": {"N": "4000000000"}, "body": {"S": body}}),
        );
    }
}

fn last_seq(w: &mut World) -> u64 {
    let streams = w
        .core
        .as_mut()
        .unwrap()
        .list_streams("us-east-1", "000000000000", Some("orders"))
        .unwrap();
    streams[0].last_seq
}

/// The items of `orders` a scan finds, every page of it.
fn count(w: &mut World) -> u64 {
    let (mut total, mut start) = (0, None);
    loop {
        let mut req = json!({"TableName": "orders", "Select": "COUNT"});
        if let Some(s) = start.take() {
            req["ExclusiveStartKey"] = s;
        }
        let page = w.ok("Scan", req);
        total += page["Count"].as_u64().unwrap();
        match page.get("LastEvaluatedKey") {
            Some(k) => start = Some(k.clone()),
            None => return total,
        }
    }
}

#[test]
fn a_table_is_measured_emptied_and_its_space_given_back() {
    let mut w = World::new();
    orders(&mut w);
    w.table("empty", false);
    fill(&mut w, "orders", 0, 300);

    let DataReply::Usage(Ok((resources, file))) = data(&mut w, DataAsk::Usage) else {
        panic!()
    };
    let of = |name: &str| resources.iter().find(|r| r.name == name).unwrap().clone();
    assert_eq!(of("orders").items, 300);
    // The item and its NEW_IMAGE stream record each hold the body (an index entry holds the key it points at).
    assert!(
        of("orders").bytes > 2 * 300 * (4 << 10),
        "{:?}",
        of("orders")
    );
    assert_eq!(
        of("orders").notes.len(),
        1,
        "the stream records are pointed at"
    );
    assert!(of("orders").notes[0].text.contains("300 stream records"));
    assert_eq!((of("empty").items, of("empty").bytes), (0, 0));
    assert!(file.bytes >= of("orders").bytes);

    // A dry run counts and removes nothing.
    let dry = Prune {
        dry_run: true,
        ..Prune::default()
    };
    let would = empty(&mut w, "orders", dry).unwrap();
    assert_eq!(
        (would.items, would.bytes, would.freed),
        (300, of("orders").bytes, 0)
    );
    assert_eq!(count(&mut w), 300);

    // Items carry no write time: an older-than prune is refused, and so is any other.
    let older = Prune {
        before_ms: Some(1),
        ..Prune::default()
    };
    assert!(matches!(
        empty(&mut w, "orders", older),
        Err(StorageError::Unsupported(_))
    ));
    assert_eq!(
        empty(&mut w, "nope", Prune::default()),
        Err(StorageError::NotFound)
    );

    let seq = last_seq(&mut w);
    let done = empty(&mut w, "orders", Prune::default()).unwrap();
    assert_eq!(done.items, 300);
    assert!(done.freed > 1 << 20, "the file shrank: {done:?}");
    assert_eq!(count(&mut w), 0);
    let by = w.ok("Query", json!({"TableName": "orders", "IndexName": "by-owner", "KeyConditionExpression": "#o = :o", "ExpressionAttributeNames": {"#o": "owner"}, "ExpressionAttributeValues": {":o": {"S": "ada"}}}));
    assert_eq!(by["Count"], 0, "the index entries went with the items");
    kept_after_a_restart(&mut w, seq);
}

/// The table keeps its settings, and a restart loads exactly what the live core holds; the stream goes on numbering from `seq`.
fn kept_after_a_restart(w: &mut World, seq: u64) {
    w.restart();
    let t = w.ok("DescribeTable", json!({"TableName": "orders"}));
    assert_eq!(
        t["Table"]["GlobalSecondaryIndexes"][0]["IndexName"],
        "by-owner"
    );
    assert_eq!(t["Table"]["StreamSpecification"]["StreamEnabled"], true);
    let ttl = w.ok("DescribeTimeToLive", json!({"TableName": "orders"}));
    assert_eq!(ttl["TimeToLiveDescription"]["TimeToLiveStatus"], "ENABLED");
    let tags = w.ok(
        "ListTagsOfResource",
        json!({"ResourceArn": "arn:aws:dynamodb:us-east-1:000000000000:table/orders"}),
    );
    assert_eq!(tags["Tags"], json!([{"Key": "team", "Value": "a"}]));
    assert_eq!(count(w), 0);
    assert_eq!(last_seq(w), seq, "a consumer's position stays valid");
    fill(w, "orders", 1000, 1);
    assert_eq!(last_seq(w), seq + 1);
    let arn = t["Table"]["LatestStreamArn"].as_str().unwrap();
    let after = w.core.as_ref().unwrap().read_stream(arn, seq, 10).unwrap();
    assert_eq!(
        after.records.len(),
        1,
        "a consumer at its old position reads the new record"
    );
}

#[test]
fn a_reset_removes_every_table_and_survives_a_restart() {
    let mut w = World::new();
    orders(&mut w);
    fill(&mut w, "orders", 0, 50);
    let dry = emptied(data(&mut w, DataAsk::Reset { dry_run: true })).unwrap();
    assert_eq!((dry.items, dry.freed), (50, 0));
    let done = emptied(data(&mut w, DataAsk::Reset { dry_run: false })).unwrap();
    assert_eq!(done.items, 50);
    assert!(done.freed > 0, "{done:?}");
    w.restart();
    assert_eq!(w.ok("ListTables", json!({}))["TableNames"], json!([]));
    let stream = w
        .core
        .as_ref()
        .unwrap()
        .disk
        .measure(Space::Stream, &[], &[0xFF; 25])
        .unwrap();
    assert_eq!(stream, (0, 0));
    // The name is free at once: a reset is a fresh start, not a delete that passes through DELETING.
    orders(&mut w);
    assert_eq!(count(&mut w), 0);
}

#[test]
fn a_backup_is_counted_on_its_own_and_emptied_by_its_name() {
    let mut w = World::new();
    w.table("orders", false);
    fill(&mut w, "orders", 0, 20);
    let arn = w.ok(
        "CreateBackup",
        json!({"TableName": "orders", "BackupName": "nightly"}),
    )["BackupDetails"]["BackupArn"]
        .as_str()
        .unwrap()
        .to_owned();
    // It outlives its table, so it is a resource of its own.
    w.ok("DeleteTable", json!({"TableName": "orders"}));
    let DataReply::Usage(Ok((resources, _))) = data(&mut w, DataAsk::Usage) else {
        panic!()
    };
    let name = arn.rsplit_once(":table/").unwrap().1.to_owned();
    assert!(name.starts_with("orders/backup/"), "{name}");
    let backup = resources.iter().find(|r| r.name == name).unwrap().clone();
    assert_eq!(backup.items, 20);
    assert!(backup.bytes > 20 * (4 << 10), "{backup:?}");
    assert!(backup.notes[0].text.contains("nightly"));

    let dry = Prune {
        dry_run: true,
        ..Prune::default()
    };
    assert_eq!(empty(&mut w, &name, dry).unwrap().items, 20);
    w.ok("DescribeBackup", json!({"BackupArn": arn}));
    let done = empty(&mut w, &name, Prune::default()).unwrap();
    assert_eq!((done.items, done.bytes), (20, backup.bytes));
    let e = w.err("DescribeBackup", json!({"BackupArn": arn}));
    assert_eq!(e.code, "BackupNotFoundException");
    assert_eq!(
        empty(&mut w, &name, Prune::default()),
        Err(StorageError::NotFound)
    );

    // A reset takes every backup with it.
    w.table("orders", false);
    fill(&mut w, "orders", 0, 5);
    w.ok(
        "CreateBackup",
        json!({"TableName": "orders", "BackupName": "again"}),
    );
    let done = emptied(data(&mut w, DataAsk::Reset { dry_run: false })).unwrap();
    assert_eq!(done.items, 10, "five items and their five backup copies");
    w.restart();
    assert_eq!(w.ok("ListBackups", json!({}))["BackupSummaries"], json!([]));
}

#[test]
fn a_stream_switched_off_takes_its_records_with_it() {
    let mut w = World::new();
    orders(&mut w);
    fill(&mut w, "orders", 0, 5);
    w.ok(
        "UpdateTable",
        json!({"TableName": "orders", "StreamSpecification": {"StreamEnabled": false}}),
    );
    let stream = w
        .core
        .as_ref()
        .unwrap()
        .disk
        .measure(Space::Stream, &[], &[0xFF; 25])
        .unwrap();
    assert_eq!(
        stream,
        (0, 0),
        "no record is left on disk with no stream to read it"
    );
    assert_eq!(count(&mut w), 5);
}

#[test]
fn a_close_after_deleting_most_items_shrinks_the_file() {
    let mut w = World::new();
    w.table("big", false);
    let body = "x".repeat(8 << 10);
    for n in 0..1100 {
        w.put(
            "big",
            json!({"pk": {"S": format!("k{n}")}, "body": {"S": body}}),
        );
    }
    for n in 0..1000 {
        w.ok(
            "DeleteItem",
            json!({"TableName": "big", "Key": {"pk": {"S": format!("k{n}")}}}),
        );
    }
    let file = w.dir.path().join(crate::disk::FILE);
    let before = std::fs::metadata(&file).unwrap().len();
    w.restart();
    let after = std::fs::metadata(&file).unwrap().len();
    assert!(
        after < before / 2,
        "the close gave the space back: {before} -> {after}"
    );
    let left = w.ok("Scan", json!({"TableName": "big", "Select": "COUNT"}));
    assert_eq!(left["Count"], 100);
}
