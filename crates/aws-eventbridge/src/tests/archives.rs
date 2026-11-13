//! Archives and replay: events of a bus kept by a pattern, put back through the rules of a window.

use serde_json::{Value, json};

use super::{START, World};

const SHOP: &str = "arn:aws:events:us-east-1:000000000000:event-bus/shop";

fn archive(w: &mut World, name: &str, extra: &Value) -> Value {
    let mut r = json!({"ArchiveName": name, "EventSourceArn": SHOP});
    for (k, v) in extra.as_object().unwrap() {
        r[k] = v.clone();
    }
    w.ok("CreateArchive", &r)
}

fn put_at(w: &mut World, source: &str, detail: &Value, seconds_ago: i64) {
    w.ok("PutEvents", &json!({"Entries": [{"EventBusName": "shop", "Source": source, "DetailType": "x", "Detail": detail.to_string(),
        "Time": START / 1000 - seconds_ago}]}));
}

#[test]
fn an_archive_keeps_the_events_its_pattern_wants_and_counts_them() {
    let mut w = World::new();
    w.bus("shop");
    let made = archive(
        &mut w,
        "a",
        &json!({"EventPattern": "{\"source\":[\"keep\"]}", "RetentionDays": 7, "Description": "d"}),
    );
    assert_eq!(
        made["ArchiveArn"],
        "arn:aws:events:us-east-1:000000000000:archive/a"
    );
    assert_eq!(made["State"], "ENABLED");
    put_at(&mut w, "keep", &json!({"n": 1}), 10);
    put_at(&mut w, "drop", &json!({"n": 2}), 10);
    put_at(&mut w, "keep", &json!({"n": 3}), 5);
    let d = w.ok("DescribeArchive", &json!({"ArchiveName": "a"}));
    assert_eq!(d["EventCount"], 2);
    assert!(d["SizeBytes"].as_u64().unwrap() > 100);
    assert_eq!(
        (
            d["RetentionDays"].clone(),
            d["Description"].clone(),
            d["State"].clone()
        ),
        (json!(7), json!("d"), json!("ENABLED"))
    );
    assert_eq!(
        w.ok("ListArchives", &json!({"EventSourceArn": SHOP}))["Archives"][0]["EventCount"],
        2
    );
    assert_eq!(
        w.ok("ListArchives", &json!({"NamePrefix": "z"}))["Archives"],
        json!([])
    );
}

#[test]
fn a_replay_puts_the_window_back_through_the_rules_marked_as_a_replay() {
    let mut w = World::new();
    w.bus("shop");
    archive(&mut w, "a", &json!({}));
    put_at(&mut w, "s", &json!({"n": 1}), 120);
    put_at(&mut w, "s", &json!({"n": 2}), 60);
    put_at(&mut w, "s", &json!({"n": 3}), 10);
    // The rule is made after the events, so they reach it only by replay.
    let rule = w.rule("shop", "r", &json!({"source": ["s"]}));
    let q = w.queue("q", &rule);
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": q}]}),
    );
    let s = START / 1000;
    let started = w.ok("StartReplay", &json!({
        "ReplayName": "rp", "EventSourceArn": "arn:aws:events:us-east-1:000000000000:archive/a",
        "EventStartTime": s - 90, "EventEndTime": s - 5, "Destination": {"Arn": SHOP}, "Description": "again"}));
    assert_eq!(started["State"], "STARTING");
    assert_eq!(
        started["ReplayArn"],
        "arn:aws:events:us-east-1:000000000000:replay/rp"
    );
    let got = w.bodies("q");
    assert_eq!(
        got.iter()
            .map(|e| e["detail"]["n"].clone())
            .collect::<Vec<_>>(),
        [json!(2), json!(3)]
    );
    assert!(got.iter().all(|e| e["replay-name"] == "rp"));
    // A replayed event is not archived again.
    assert_eq!(
        w.ok("DescribeArchive", &json!({"ArchiveName": "a"}))["EventCount"],
        3
    );
    let d = w.ok("DescribeReplay", &json!({"ReplayName": "rp"}));
    assert_eq!(d["State"], "COMPLETED");
    assert_eq!(d["Destination"]["Arn"], SHOP);
    assert_eq!(d["Description"], "again");
    assert_eq!(
        d["EventLastReplayedTime"].as_f64().unwrap(),
        (s - 10) as f64
    );
    assert_eq!(
        w.ok("ListReplays", &json!({}))["Replays"][0]["ReplayName"],
        "rp"
    );
    assert_eq!(
        w.ok("ListReplays", &json!({"State": "RUNNING"}))["Replays"],
        json!([])
    );
}

#[test]
fn a_replay_can_be_limited_to_named_rules() {
    let mut w = World::new();
    w.bus("shop");
    archive(&mut w, "a", &json!({}));
    put_at(&mut w, "s", &json!({}), 30);
    let one = w.rule("shop", "one", &json!({"source": ["s"]}));
    let two = w.rule("shop", "two", &json!({"source": ["s"]}));
    for (name, rule) in [("one", &one), ("two", &two)] {
        let q = w.queue(&format!("q-{name}"), rule);
        w.ok(
            "PutTargets",
            &json!({"Rule": name, "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": q}]}),
        );
    }
    let s = START / 1000;
    w.ok("StartReplay", &json!({"ReplayName": "rp", "EventSourceArn": "arn:aws:events:us-east-1:000000000000:archive/a",
        "EventStartTime": s - 60, "EventEndTime": s, "Destination": {"Arn": SHOP, "FilterArns": [two]}}));
    assert!(w.drain("q-one").is_empty());
    assert_eq!(w.drain("q-two").len(), 1);
}

#[test]
fn the_wording_of_the_archive_and_replay_refusals() {
    let mut w = World::new();
    w.bus("shop");
    archive(&mut w, "a", &json!({}));
    assert_eq!(
        w.err(
            "CreateArchive",
            &json!({"ArchiveName": "a", "EventSourceArn": SHOP})
        )
        .0,
        "ResourceAlreadyExistsException"
    );
    assert_eq!(w.err("CreateArchive", &json!({"ArchiveName": "b", "EventSourceArn": "arn:aws:events:us-east-1:000000000000:event-bus/none"})).0, "ResourceNotFoundException");
    assert_eq!(
        w.err(
            "CreateArchive",
            &json!({"ArchiveName": "b", "EventSourceArn": SHOP, "EventPattern": "{"})
        )
        .0,
        "InvalidEventPatternException"
    );
    assert_eq!(
        w.err("DescribeArchive", &json!({"ArchiveName": "none"})),
        (
            "ResourceNotFoundException".into(),
            "Archive none does not exist.".into()
        )
    );
    let s = START / 1000;
    let replay = |name: &str, start: i64, end: i64| {
        json!({"ReplayName": name, "EventSourceArn": "arn:aws:events:us-east-1:000000000000:archive/a",
        "EventStartTime": start, "EventEndTime": end, "Destination": {"Arn": SHOP}})
    };
    assert_eq!(
        w.err("StartReplay", &replay("r", s, s - 10)).0,
        "ValidationException"
    );
    w.ok("StartReplay", &replay("r", s - 10, s));
    assert_eq!(
        w.err("StartReplay", &replay("r", s - 10, s)).0,
        "ResourceAlreadyExistsException"
    );
    let mut elsewhere = replay("r2", s - 10, s);
    elsewhere["Destination"]["Arn"] =
        json!("arn:aws:events:us-east-1:000000000000:event-bus/default");
    assert_eq!(w.err("StartReplay", &elsewhere).0, "ValidationException");
    assert_eq!(
        w.err("DescribeReplay", &json!({"ReplayName": "none"})).0,
        "ResourceNotFoundException"
    );
}

#[test]
fn an_archive_is_updated_and_deleted_with_its_events() {
    let mut w = World::new();
    w.bus("shop");
    archive(&mut w, "a", &json!({}));
    put_at(&mut w, "s", &json!({}), 5);
    w.ok("UpdateArchive", &json!({"ArchiveName": "a", "Description": "new", "RetentionDays": 1, "EventPattern": "{\"source\":[\"s\"]}"}));
    let d = w.ok("DescribeArchive", &json!({"ArchiveName": "a"}));
    assert_eq!(
        (d["Description"].clone(), d["RetentionDays"].clone()),
        (json!("new"), json!(1))
    );
    w.ok("DeleteArchive", &json!({"ArchiveName": "a"}));
    assert_eq!(
        w.err("DescribeArchive", &json!({"ArchiveName": "a"})).0,
        "ResourceNotFoundException"
    );
    // Made again, it starts empty.
    archive(&mut w, "a", &json!({}));
    assert_eq!(
        w.ok("DescribeArchive", &json!({"ArchiveName": "a"}))["EventCount"],
        0
    );
    assert!(
        w.core
            .archived
            .scan(None, "0", "z", 10, false)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn old_events_leave_an_archive_when_its_retention_has_passed() {
    let mut w = World::new();
    w.bus("shop");
    archive(&mut w, "a", &json!({"RetentionDays": 1}));
    put_at(&mut w, "s", &json!({"n": 1}), 0);
    w.clock.advance(2 * 86_400_000);
    // The event after the retention is the occasion to look for old ones.
    w.ok("PutEvents", &json!({"Entries": [{"EventBusName": "shop", "Source": "s", "DetailType": "x", "Detail": "{}", "Time": (START + 2 * 86_400_000) / 1000}]}));
    assert_eq!(
        w.ok("DescribeArchive", &json!({"ArchiveName": "a"}))["EventCount"],
        1
    );
}
