//! `ConsoleView::log_facts` and `producers`: what the Log and Producers pages say, held to what the broker then does.
#![cfg(feature = "inspect")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::collections::BTreeSet;

use kafka_auth::Principal;
use kafka_broker::{Caller, CleanerState};
use kafka_wire::batch::Batches;
use support::{keyed, records, rig, rig_with, topic, with_config};

fn console() -> Caller {
    Caller::new(Principal::console(), Some("127.0.0.1".parse().unwrap()))
}

fn compacting(ratio: &str) -> kafka_wire::messages::create_topics_request::CreatableTopic {
    let t = with_config(topic("c", 1), "cleanup.policy", "compact");
    let t = with_config(t, "segment.bytes", "1024");
    with_config(t, "min.cleanable.dirty.ratio", ratio)
}

async fn fill(rig: &support::Rig, n: usize, keys: usize) {
    for i in 0..n {
        let one = keyed(&[(&format!("k{}", i % keys), Some(&format!("v{i}")))]);
        assert_eq!(rig.produce(-1, "c", 0, one).await.error_code, 0);
    }
}

/// The offsets the topic serves from the start, read as a consumer does.
async fn served(rig: &support::Rig) -> BTreeSet<i64> {
    let (mut at, end) = (0, rig.list_offset(7, "c", 0, -1).await.offset);
    let mut out = BTreeSet::new();
    while at < end {
        let data = rig.fetch_now(&[("c", 0, at, 1 << 20)]).await.remove(0);
        for b in Batches::new(data.records.expect("records")) {
            let b = b.unwrap();
            b.each_record(1 << 20, |s| {
                if s.offset >= at {
                    out.insert(s.offset);
                }
            })
            .unwrap();
            at = b.next_offset().unwrap().get();
        }
    }
    out
}

/// CON-501: the verdict the page shows is the planner's, and a round then does what it said: due means a round ran and
/// replaced segments, not due means nothing changed.
#[tokio::test]
async fn the_verdict_is_what_the_cleaner_then_does() {
    // Dirty over the ratio (everything is dirty on a log never cleaned), then not over it (a few new records beside a large
    // clean part), then held back by a minimum lag (every segment is too new).
    let rig = rig();
    rig.create(vec![compacting("0.5")], false).await;
    fill(&rig, 80, 40).await;
    let first = agree(&rig).await;
    assert!(first.0, "a log never cleaned is due");
    assert!(first.1.last.is_some() && first.1.state == CleanerState::Watching);
    fill(&rig, 16, 40).await;
    let second = agree(&rig).await;
    assert!(
        !second.0,
        "a few records beside a clean log are not over half: {:?}",
        second.1
    );
    assert_eq!(
        second.1.reading.limit,
        kafka_storage::compact::Limit::Active
    );
    assert!(second.1.reading.ratio().is_some_and(|r| r < 0.5));

    let rig = support::rig();
    let lagged = with_config(compacting("0.01"), "min.compaction.lag.ms", "1000000000");
    let lagged = with_config(lagged, "message.timestamp.type", "LogAppendTime");
    rig.create(vec![lagged], false).await;
    fill(&rig, 40, 5).await;
    let held = agree(&rig).await;
    assert!(!held.0, "{:?}", held.1);
    assert_eq!(held.1.reading.limit, kafka_storage::compact::Limit::TooNew);
}

/// Looks at the page's verdict, runs the cleaner, and checks that the round happened exactly if the verdict said so: whether
/// it was due, and the compaction facts after.
async fn agree(rig: &support::Rig) -> (bool, kafka_broker::Compaction) {
    let view = rig.broker.console_view();
    let before = view.log_facts(&console(), "c", 0).await.unwrap();
    let c = before.compaction.clone().expect("a compacted topic");
    let (due, round_before) = (c.verdict.is_some(), c.last);
    let shape = |f: &kafka_broker::LogFacts| {
        (
            f.checkpoint,
            f.segments
                .iter()
                .map(|s| (s.base, s.bytes))
                .collect::<Vec<_>>(),
        )
    };
    let was = shape(&before);
    rig.broker.check_cleaner().await;
    let after = view.log_facts(&console(), "c", 0).await.unwrap();
    let ac = after.compaction.clone().unwrap();
    assert!(ac.looked_at_ms.is_some(), "the cleaner has looked");
    if due {
        let round = ac.last.expect("a round ran");
        assert_ne!(Some(round), round_before, "a new round is noted");
        assert!(round.replaced > 0 && round.kept < round.read, "{round:?}");
        assert_eq!(
            round.clean_to, after.checkpoint,
            "the note and the file agree"
        );
        assert_ne!(shape(&after), was, "the segments were rewritten");
    } else {
        assert_eq!(ac.last, round_before, "no round ran");
        assert_eq!(shape(&after), was, "nothing changed");
    }
    (due, ac)
}

/// CON-502: the ruler is drawn from a real compacted partition: what is in a gap is exactly what a consumer no longer gets.
#[tokio::test]
async fn the_gaps_are_exactly_the_offsets_the_cleaner_removed() {
    let rig = rig();
    rig.create(vec![compacting("0.01")], false).await;
    fill(&rig, 60, 5).await;
    rig.broker.check_cleaner().await;
    let facts = rig
        .broker
        .console_view()
        .log_facts(&console(), "c", 0)
        .await
        .unwrap();
    let gone: BTreeSet<i64> = facts
        .segments
        .iter()
        .flat_map(|s| s.gaps.iter().flat_map(|(f, n)| *f..*f + *n))
        .collect();
    let served = served(&rig).await;
    let want: BTreeSet<i64> = (0..60).filter(|o| !served.contains(o)).collect();
    assert_eq!(gone, want);
    assert!(
        gone.len() >= 40,
        "most of 60 records over 5 keys are superseded: {}",
        gone.len()
    );
    assert!(!facts.partial);
    // The segments cover the log with no hole of their own: each begins where the one before ends.
    for pair in facts.segments.windows(2) {
        assert_eq!(pair[0].next, pair[1].base);
    }
    assert_eq!((facts.start, facts.end, facts.last_stable), (0, 60, 60));
}

/// CON-503: a deletion a round kept is listed with the time it goes; a delete-policy topic has no cleaner side.
#[tokio::test]
async fn a_kept_deletion_is_listed_with_its_horizon() {
    let rig = rig();
    let t = with_config(compacting("0.01"), "delete.retention.ms", "86400000");
    rig.create(vec![t], false).await;
    assert_eq!(
        rig.produce(-1, "c", 0, keyed(&[("gone", Some("1"))]))
            .await
            .error_code,
        0
    );
    assert_eq!(
        rig.produce(-1, "c", 0, keyed(&[("gone", None)]))
            .await
            .error_code,
        0
    );
    fill(&rig, 30, 4).await;
    rig.broker.check_cleaner().await;
    let facts = rig
        .broker
        .console_view()
        .log_facts(&console(), "c", 0)
        .await
        .unwrap();
    let [t] = facts.tombstones.as_slice() else {
        panic!("{:?}", facts.tombstones);
    };
    assert_eq!((t.key.as_slice(), t.offset), (b"gone".as_slice(), 1));
    assert!(t.goes_after_ms > facts.now_ms, "a day away");
    assert!(t.goes_after_ms - t.written_ms >= 86_000_000);
}

/// CON-504: retention's side: the broker's own rule says how many segments the next pass deletes, and the pass is noted.
#[tokio::test]
async fn retention_says_what_the_next_pass_does_and_what_the_last_one_did() {
    let rig = rig();
    let t = with_config(topic("d", 1), "segment.bytes", "1024");
    let t = with_config(t, "retention.ms", "1");
    rig.create(vec![t], false).await;
    for _ in 0..30 {
        assert_eq!(rig.produce(-1, "d", 0, records(5)).await.error_code, 0);
    }
    let view = rig.broker.console_view();
    let before = view.log_facts(&console(), "d", 0).await.unwrap();
    assert!(before.compaction.is_none());
    let r = before.retention.clone().expect("a delete topic");
    assert!(r.last_pass.is_none(), "no pass yet");
    assert_eq!(r.max_age_ms, Some(1));
    std::thread::sleep(std::time::Duration::from_millis(5));
    let before = view.log_facts(&console(), "d", 0).await.unwrap();
    let r = before.retention.clone().unwrap();
    assert!(
        r.would_delete > 0,
        "every segment is past a millisecond: {r:?}"
    );
    let start = before.start;
    rig.broker.check_logs().await;
    let after = view.log_facts(&console(), "d", 0).await.unwrap();
    let r = after.retention.unwrap();
    let pass = r.last_pass.expect("the pass is noted");
    let [moved] = pass.deleted.as_slice() else {
        panic!("{pass:?}");
    };
    assert_eq!(
        (moved.topic.as_str(), moved.partition, moved.from),
        ("d", 0, start)
    );
    assert_eq!(
        moved.to, after.start,
        "the note says where the log start went"
    );
    assert!(after.start > start);
    assert_eq!((pass.looked, r.moved_total), (1, 1));
    assert!(r.moved_at_ms.is_some());
}

/// CON-505: a partition never written has nothing: no files, one honest empty answer, and looking opens nothing.
#[tokio::test]
async fn an_empty_partition_says_so_and_looking_opens_nothing() {
    let rig = rig();
    rig.create(vec![topic("e", 2)], false).await;
    let view = rig.broker.console_view();
    let facts = view.log_facts(&console(), "e", 1).await.unwrap();
    assert!(facts.never_written && facts.segments.is_empty());
    assert_eq!(
        (facts.start, facts.end, facts.last_stable, facts.partitions),
        (0, 0, 0, 2)
    );
    assert!(facts.policy.delete && !facts.policy.compact);
    assert_eq!(rig.broker.open_partitions(), 0);
    // A partition that was written to is looked at in full, and looking leaves the set of open partitions as it was.
    rig.produce(1, "e", 0, records(2)).await;
    assert_eq!(rig.broker.open_partitions(), 1);
    let facts = view.log_facts(&console(), "e", 0).await.unwrap();
    assert!(!facts.never_written);
    assert_eq!((facts.end, facts.segments.len()), (2, 1));
    assert_eq!(rig.broker.open_partitions(), 1);
    let again = view.log_facts(&console(), "e", 1).await.unwrap();
    assert!(again.never_written);
    assert_eq!(rig.broker.open_partitions(), 1);
}

/// CON-505b: a partition closed to make room is looked at from its files and let go again.
#[tokio::test]
async fn a_closed_partition_is_looked_at_and_closed_again() {
    let rig = rig_with(|c| c.max_open_partitions = 1);
    rig.create(vec![topic("t", 2)], false).await;
    rig.produce(1, "t", 0, records(4)).await;
    rig.produce(1, "t", 1, records(1)).await;
    assert_eq!(
        rig.broker.open_partitions(),
        1,
        "partition 0 was closed for room"
    );
    let facts = rig
        .broker
        .console_view()
        .log_facts(&console(), "t", 0)
        .await
        .unwrap();
    assert_eq!((facts.end, facts.never_written), (4, false));
    // Making room for it closed the other one; it was then let go itself, as nothing else held it.
    assert_eq!(rig.broker.open_partitions(), 0, "and it is closed again");
}

/// CON-506: Describe is the right, and a partition that is not there is no partition.
#[tokio::test]
async fn the_view_obeys_describe_and_does_not_say_which_partitions_exist() {
    let made = rig();
    made.create(vec![topic("s", 1)], false).await;
    let rig = support::open_on_with(made.fs.process_restart(), |c| c.acl_default_allow = false);
    let view = rig.broker.console_view();
    let nobody = Caller::anonymous(Some("127.0.0.1".parse().unwrap()));
    assert!(view.log_facts(&nobody, "s", 0).await.is_none());
    assert!(view.producers(&nobody, "s", 0).await.is_none());
    assert!(view.log_facts(&console(), "s", 0).await.is_some());
    assert!(
        view.log_facts(&console(), "s", 1).await.is_none(),
        "no such partition"
    );
    assert!(view.log_facts(&console(), "nope", 0).await.is_none());
    assert!(view.producers(&console(), "s", -1).await.is_none());
}
