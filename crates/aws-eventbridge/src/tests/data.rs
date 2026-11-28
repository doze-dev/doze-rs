//! `doze-aws data` on EventBridge: an archive's size is its events, emptying keeps the archive and its counts agree, an older-than
//! prune keeps the newer events, a reset removes the definitions and the default bus comes back as on a fresh start.

use std::sync::Arc;

use aws_testkit::TempDir;
use aws_wire::storage::{Emptied, Prune, StorageError};
use doze_runtime::{ManualClock, Tasks};
use serde_json::json;

use super::{START, World};
use crate::core::Core;
use crate::storage::{DataAsk, DataReply};

const SHOP: &str = "arn:aws:events:us-east-1:000000000000:event-bus/shop";

fn open(dir: &TempDir) -> Core {
    Core::open_in(
        dir.path(),
        Arc::new(ManualClock::at(START)),
        None,
        Tasks::new(),
    )
    .unwrap()
}

fn reopen(w: &mut World, dir: &TempDir) {
    w.core.db.take().unwrap().close();
    w.core = open(dir);
}

/// `count` events of 4 KiB, the oldest `count` seconds ago.
fn fill(w: &mut World, count: i64) {
    let detail = json!({"pad": "x".repeat(4 << 10)}).to_string();
    for chunk in (1..=count).collect::<Vec<_>>().chunks(10) {
        let entries: Vec<_> = chunk
            .iter()
            .map(|ago| json!({"EventBusName": "shop", "Source": "s", "DetailType": "x", "Detail": detail, "Time": START / 1000 - ago}))
            .collect();
        w.ok("PutEvents", &json!({"Entries": entries}));
    }
    w.deliver();
}

fn ask(w: &mut World, ask: DataAsk) -> Result<Emptied, StorageError> {
    match data_now(&mut w.core, "us-east-1", ask) {
        DataReply::Emptied(done) => done,
        other => panic!("{other:?}"),
    }
}

fn empty(name: &str, prune: Prune) -> DataAsk {
    DataAsk::Empty {
        name: name.into(),
        prune,
    }
}

#[test]
fn an_archive_is_measured_emptied_and_its_space_given_back() {
    let dir = TempDir::new("eventbridge-data");
    let mut w = World::new();
    w.core = open(&dir);
    w.bus("shop");
    w.rule("shop", "r", &json!({"source": ["s"]}));
    w.ok(
        "CreateArchive",
        &json!({"ArchiveName": "a", "EventSourceArn": SHOP}),
    );
    fill(&mut w, 300);

    let DataReply::Usage(resources, file) = data_now(&mut w.core, "us-east-1", DataAsk::Usage)
    else {
        panic!()
    };
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].items, 300);
    assert!(resources[0].bytes > 300 * (4 << 10));
    assert_eq!(resources[0].notes.len(), 1, "no retention: kept forever");
    assert!(file.bytes >= resources[0].bytes);

    let dry = Prune {
        dry_run: true,
        ..Prune::default()
    };
    let would = ask(&mut w, empty("a", dry)).unwrap();
    assert_eq!((would.items, would.freed), (300, 0));

    // Before 100 seconds ago: the 100 newest stay (99 strictly after, and the one at the cut).
    let older = Prune {
        before_ms: Some(START - 100_000),
        ..Prune::default()
    };
    let done = ask(&mut w, empty("a", older)).unwrap();
    assert_eq!(done.items, 200);
    assert!(done.freed > 0, "the file shrank: {done:?}");
    let d = w.ok("DescribeArchive", &json!({"ArchiveName": "a"}));
    assert_eq!(d["EventCount"], 100);

    // The archive, the bus and the rule stay; what is left survives a restart.
    reopen(&mut w, &dir);
    let d = w.ok("DescribeArchive", &json!({"ArchiveName": "a"}));
    assert_eq!(d["EventCount"], 100);
    w.ok(
        "DescribeRule",
        &json!({"Name": "r", "EventBusName": "shop"}),
    );
    let done = ask(&mut w, empty("a", Prune::default())).unwrap();
    assert_eq!(done.items, 100);
    let d = w.ok("DescribeArchive", &json!({"ArchiveName": "a"}));
    assert_eq!(
        (d["EventCount"].clone(), d["SizeBytes"].clone()),
        (json!(0), json!(0))
    );

    let prefix = Prune {
        prefix: Some("a".into()),
        ..Prune::default()
    };
    assert!(matches!(
        ask(&mut w, empty("a", prefix)),
        Err(StorageError::Unsupported(_))
    ));
    assert_eq!(
        ask(&mut w, empty("nope", Prune::default())),
        Err(StorageError::NotFound)
    );
}

#[test]
fn a_reset_removes_everything_and_the_default_bus_comes_back() {
    let dir = TempDir::new("eventbridge-data-reset");
    let mut w = World::new();
    w.core = open(&dir);
    w.bus("shop");
    w.rule("shop", "r", &json!({"source": ["s"]}));
    w.ok(
        "CreateArchive",
        &json!({"ArchiveName": "a", "EventSourceArn": SHOP}),
    );
    fill(&mut w, 20);
    w.ok(
        "PutRule",
        &json!({"Name": "on-default", "EventPattern": "{\"source\":[\"x\"]}"}),
    );
    let done = ask(&mut w, DataAsk::Reset { dry_run: false }).unwrap();
    assert_eq!(done.items, 20);
    reopen(&mut w, &dir);
    let buses = w.ok("ListEventBuses", &json!({}));
    let names: Vec<&str> = buses["EventBuses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["Name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["default"]);
    assert!(
        w.ok("ListRules", &json!({}))["Rules"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        w.ok("ListArchives", &json!({}))["Archives"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    w.bus("shop");
}

/// A data question, on a runtime of one thread as the server's (a compaction goes to its blocking pool).
fn data_now(
    core: &mut crate::core::Core,
    region: &str,
    ask: crate::storage::DataAsk,
) -> crate::storage::DataReply {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(core.data(region, ask))
}
