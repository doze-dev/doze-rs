//! A replay runs over time and can be cancelled while it is STARTING or RUNNING.

use serde_json::{Value, json};

use super::{START, World};

const SHOP: &str = "arn:aws:events:us-east-1:000000000000:event-bus/shop";
const ARCHIVE: &str = "arn:aws:events:us-east-1:000000000000:archive/a";

/// A bus with an archive holding `n` events of the last minutes, and a rule made afterwards that sends them to queue `q`.
fn world_with(n: usize) -> World {
    let mut w = World::new();
    w.bus("shop");
    w.ok(
        "CreateArchive",
        &json!({"ArchiveName": "a", "EventSourceArn": SHOP}),
    );
    let entries: Vec<Value> = (0..n)
        .map(|i| json!({"EventBusName": "shop", "Source": "s", "DetailType": "x", "Detail": json!({"n": i}).to_string(),
            "Time": START / 1000 - 100}))
        .collect();
    for chunk in entries.chunks(10) {
        w.ok("PutEvents", &json!({"Entries": chunk}));
        w.deliver();
    }
    let rule = w.rule("shop", "r", &json!({"source": ["s"]}));
    let q = w.queue("q", &rule);
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": q}]}),
    );
    w
}

fn start(w: &mut World, name: &str) -> Value {
    let s = START / 1000;
    w.ok(
        "StartReplay",
        &json!({"ReplayName": name, "EventSourceArn": ARCHIVE,
        "EventStartTime": s - 200, "EventEndTime": s, "Destination": {"Arn": SHOP}}),
    )
}

/// Every message in queue `q`, taken off it.
fn count(w: &World) -> usize {
    let mut total = 0;
    loop {
        let got = w.drain("q");
        if got.is_empty() {
            return total;
        }
        for m in &got {
            w.peer(
                "sqs",
                "DeleteMessage",
                &json!({"QueueUrl": World::queue_url("q"), "ReceiptHandle": m["ReceiptHandle"]}),
            )
            .unwrap();
        }
        total += got.len();
    }
}

fn state(w: &mut World, name: &str) -> Value {
    w.ok("DescribeReplay", &json!({"ReplayName": name}))
}

#[test]
fn a_starting_replay_is_cancelled_before_it_puts_anything_back() {
    let mut w = world_with(3);
    start(&mut w, "rp");
    let cancelled = w.ok("CancelReplay", &json!({"ReplayName": "rp"}));
    assert_eq!(cancelled["State"], "CANCELLING");
    assert_eq!(
        cancelled["ReplayArn"],
        "arn:aws:events:us-east-1:000000000000:replay/rp"
    );
    assert_eq!(state(&mut w, "rp")["State"], "CANCELLING");
    w.finish_replays();
    let d = state(&mut w, "rp");
    assert_eq!(d["State"], "CANCELLED");
    assert!(
        d.get("StateReason").is_none(),
        "a cancelled replay has no reason (recorded)"
    );
    assert!(d["ReplayEndTime"].is_number());
    assert_eq!(count(&w), 0);
    assert_eq!(
        w.ok("ListReplays", &json!({"State": "CANCELLED"}))["Replays"][0]["ReplayName"],
        "rp"
    );
    // A cancelled replay keeps its name.
    assert_eq!(
        w.err(
            "StartReplay",
            &json!({"ReplayName": "rp", "EventSourceArn": ARCHIVE,
            "EventStartTime": 1, "EventEndTime": 2, "Destination": {"Arn": SHOP}})
        )
        .0,
        "ResourceAlreadyExistsException"
    );
}

#[test]
fn a_running_replay_stops_where_it_is_when_cancelled() {
    let mut w = world_with(250);
    start(&mut w, "rp");
    // The first step puts back one batch and the replay is RUNNING.
    let first = w.core.next_replay_step().unwrap();
    assert_eq!(first, START + 1_000);
    w.wake_at(first);
    assert_eq!(state(&mut w, "rp")["State"], "RUNNING");
    assert!(state(&mut w, "rp")["EventLastReplayedTime"].is_number());
    assert_eq!(count(&w), 100);
    w.ok("CancelReplay", &json!({"ReplayName": "rp"}));
    w.finish_replays();
    assert_eq!(state(&mut w, "rp")["State"], "CANCELLED");
    assert_eq!(count(&w), 0, "nothing more is put back after a cancel");
}

#[test]
fn a_replay_that_is_not_going_cannot_be_cancelled() {
    let mut w = world_with(2);
    assert_eq!(
        w.err("CancelReplay", &json!({"ReplayName": "none"})),
        (
            "ResourceNotFoundException".into(),
            "Replay none does not exist.".into()
        )
    );
    start(&mut w, "done");
    w.finish_replays();
    assert_eq!(state(&mut w, "done")["State"], "COMPLETED");
    assert_eq!(count(&w), 2);
    assert_eq!(
        w.err("CancelReplay", &json!({"ReplayName": "done"})).0,
        "IllegalStatusException"
    );
    start(&mut w, "twice");
    w.ok("CancelReplay", &json!({"ReplayName": "twice"}));
    assert_eq!(
        w.err("CancelReplay", &json!({"ReplayName": "twice"})).0,
        "IllegalStatusException"
    );
    // With nothing going there is no deadline: no timer is held for replays.
    w.finish_replays();
    assert_eq!(w.core.next_replay_step(), None);
}

#[test]
fn a_replay_whose_archive_goes_fails() {
    let mut w = world_with(2);
    start(&mut w, "rp");
    w.ok("DeleteArchive", &json!({"ArchiveName": "a"}));
    w.finish_replays();
    let d = state(&mut w, "rp");
    assert_eq!(d["State"], "FAILED");
    assert!(d["StateReason"].is_string());
    assert_eq!(count(&w), 0);
}
