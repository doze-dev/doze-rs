//! CreatePartitions as Apache Kafka 4.3.1 answers it: a topic given more partitions has them at once, for producers,
//! consumers and Metadata; and every refusal is Kafka's, in Kafka's order and words.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{open_on, records, rig, topic};

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const INVALID_PARTITIONS: i16 = 37;
const INVALID_REPLICA_ASSIGNMENT: i16 = 39;
const INVALID_REQUEST: i16 = 42;

/// A topic of a request: name, new count, assignments if any.
type Ask = (&'static str, i32, Option<Vec<Vec<i32>>>);

fn ok(name: &str) -> (String, i16, Option<String>) {
    (name.to_owned(), 0, None)
}

fn refused(name: &str, code: i16, message: &str) -> (String, i16, Option<String>) {
    (name.to_owned(), code, Some(message.to_owned()))
}

async fn partitions_of(rig: &support::Rig, name: &str) -> usize {
    rig.metadata(12, Some(&[name]), false).await.topics[0]
        .partitions
        .len()
}

#[tokio::test]
async fn a_topic_given_more_partitions_has_them_for_metadata_and_for_producers_and_they_start_empty()
 {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    assert_eq!(rig.produce(-1, "t", 1, records(3)).await.error_code, 0);
    assert_eq!(
        rig.create_partitions(3, false, &[("t", 5, None)]).await,
        [ok("t")]
    );
    assert_eq!(partitions_of(&rig, "t").await, 5);
    for p in 2..5 {
        assert_eq!(rig.list_offset(7, "t", p, -1).await.offset, 0);
        assert_eq!(rig.produce(-1, "t", p, records(2)).await.error_code, 0);
    }
    // The old partitions are as they were.
    assert_eq!(rig.list_offset(7, "t", 1, -1).await.offset, 3);
    assert_eq!(
        rig.list_offset(7, "t", 5, -1).await.error_code,
        UNKNOWN_TOPIC_OR_PARTITION
    );
}

#[tokio::test]
async fn the_refusals_are_kafkas_in_kafkas_words() {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    let ask = |topics: Vec<Ask>| {
        let rig = &rig;
        async move { rig.create_partitions(3, false, &topics).await }
    };
    assert_eq!(
        ask(vec![("t", 3, None)]).await,
        [refused(
            "t",
            INVALID_PARTITIONS,
            "Topic already has 3 partition(s)."
        )]
    );
    assert_eq!(
        ask(vec![("t", 2, None)]).await,
        [refused(
            "t",
            INVALID_PARTITIONS,
            "The topic t currently has 3 partition(s); 2 would not be an increase."
        )]
    );
    assert_eq!(
        ask(vec![("ghost", 4, None)]).await,
        [("ghost".to_owned(), UNKNOWN_TOPIC_OR_PARTITION, None)]
    );
    assert_eq!(
        ask(vec![("t", 5, Some(vec![vec![1]]))]).await,
        [refused(
            "t",
            INVALID_REPLICA_ASSIGNMENT,
            "Attempted to add 2 additional partition(s), but only 1 assignment(s) were specified."
        )]
    );
    assert_eq!(
        partitions_of(&rig, "t").await,
        3,
        "nothing of a refused request was done"
    );
}

#[tokio::test]
async fn a_manual_assignment_is_judged_as_kafka_judges_it_for_one_broker() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let with = |ids: Vec<Vec<i32>>| {
        let rig = &rig;
        async move {
            rig.create_partitions(3, false, &[("t", 1 + ids.len() as i32, Some(ids))])
                .await
        }
    };
    let bad = |m: &str| vec![refused("t", INVALID_REPLICA_ASSIGNMENT, m)];
    assert_eq!(
        with(vec![vec![]]).await,
        bad("The manual partition assignment includes an empty replica list.")
    );
    assert_eq!(
        with(vec![vec![2]]).await,
        bad("The manual partition assignment includes broker 2, but no such broker is registered.")
    );
    assert_eq!(
        with(vec![vec![1, 1]]).await,
        bad("The manual partition assignment includes the broker 1 more than once.")
    );
    assert_eq!(partitions_of(&rig, "t").await, 1);
    assert_eq!(with(vec![vec![1], vec![1]]).await, [ok("t")]);
    assert_eq!(partitions_of(&rig, "t").await, 3);
}

#[tokio::test]
async fn a_name_listed_twice_is_refused_first_and_once_and_the_others_still_go_through() {
    let rig = rig();
    rig.create(vec![topic("a", 1), topic("b", 1)], false).await;
    let got = rig
        .create_partitions(3, false, &[("a", 2, None), ("b", 2, None), ("a", 3, None)])
        .await;
    assert_eq!(
        got,
        [
            refused("a", INVALID_REQUEST, "Duplicate topic name."),
            ok("b")
        ]
    );
    assert_eq!(
        partitions_of(&rig, "a").await,
        1,
        "neither entry of the duplicate was acted on"
    );
    assert_eq!(partitions_of(&rig, "b").await, 2);
}

#[tokio::test]
async fn a_validation_changes_nothing_but_judges_everything() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    assert_eq!(
        rig.create_partitions(0, true, &[("t", 4, None)]).await,
        [ok("t")]
    );
    assert_eq!(partitions_of(&rig, "t").await, 1);
    assert_eq!(
        rig.create_partitions(0, true, &[("t", 1, None)]).await,
        [refused(
            "t",
            INVALID_PARTITIONS,
            "Topic already has 1 partition(s)."
        )]
    );
}

#[tokio::test]
async fn more_than_the_brokers_limit_is_refused() {
    let rig = support::rig_limited(4);
    rig.create(vec![topic("t", 1)], false).await;
    let got = rig.create_partitions(3, false, &[("t", 5, None)]).await;
    assert_eq!(got[0].1, INVALID_PARTITIONS);
    assert_eq!(
        rig.create_partitions(3, false, &[("t", 4, None)]).await,
        [ok("t")]
    );
}

#[tokio::test]
async fn every_offered_version_answers_and_the_new_partitions_are_there_after_a_restart() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    for (v, count) in [(0, 2), (1, 3), (2, 4), (3, 5)] {
        assert_eq!(
            rig.create_partitions(v, false, &[("t", count, None)]).await,
            [ok("t")]
        );
    }
    assert_eq!(rig.produce(-1, "t", 4, records(2)).await.error_code, 0);
    let again = open_on(rig.fs.process_restart());
    assert_eq!(partitions_of(&again, "t").await, 5);
    assert_eq!(again.list_offset(7, "t", 4, -1).await.offset, 2);
}

/// A stop (a kill, or a power cut) after each operation of the change: the topic has its old count or its new one, never
/// something between, and every partition it has can be written to and read, after the restart and after another.
#[tokio::test]
async fn a_stop_at_any_point_leaves_the_topic_with_its_old_count_or_its_new_and_every_partition_usable()
 {
    use kafka_storage::sim::{Cut, DataCut, NameCut};
    let clean = rig();
    clean.create(vec![topic("t", 2)], false).await;
    let before = clean.fs.ops();
    assert_eq!(
        clean.create_partitions(3, false, &[("t", 5, None)]).await,
        [ok("t")]
    );
    let steps = clean.fs.ops() - before;
    assert!(steps >= 3, "{steps}");
    let mut seen = std::collections::BTreeSet::new();
    for stop in 0..=steps {
        for power in [false, true] {
            let rig = rig();
            rig.create(vec![topic("t", 2)], false).await;
            rig.fs.halt_after(stop);
            let answer = rig.create_partitions(3, false, &[("t", 5, None)]).await;
            let mut disk = if power {
                rig.fs.power_cut(Cut {
                    data: DataCut::Drop,
                    names: NameCut::Durable,
                    seed: stop,
                })
            } else {
                rig.fs.process_restart()
            };
            let what = format!("stop {stop}/{steps}, power {power}, answered {answer:?}");
            for round in 0..2 {
                let again = open_on(disk.process_restart());
                let n = partitions_of(&again, "t").await;
                assert!(n == 2 || n == 5, "{what}: {n} partitions");
                if answer == [ok("t")] {
                    assert_eq!(n, 5, "{what}: acknowledged, so there");
                }
                seen.insert(n);
                if round == 0 {
                    // The counted partitions are all on the disk before anything has touched them.
                    let dirs = again.dirs();
                    for p in 0..n {
                        assert!(
                            dirs.contains(&format!("t-{p}")),
                            "{what}: no directory for partition {p}: {dirs:?}"
                        );
                    }
                }
                for p in 0..i32::try_from(n).unwrap() {
                    let end = again.list_offset(7, "t", p, -1).await.offset;
                    assert_eq!(
                        again.produce(-1, "t", p, records(1)).await.error_code,
                        0,
                        "{what} round {round} partition {p}"
                    );
                    assert_eq!(
                        again.list_offset(7, "t", p, -1).await.offset,
                        end + 1,
                        "{what} partition {p}"
                    );
                }
                // The producer, finding no answer, asks again: either it is done or it was already.
                let retry = again.create_partitions(3, false, &[("t", 5, None)]).await;
                assert!(
                    retry == [ok("t")] || retry[0].1 == INVALID_PARTITIONS,
                    "{what}: {retry:?}"
                );
                assert_eq!(partitions_of(&again, "t").await, 5, "{what}");
                disk = again.fs.process_restart();
            }
        }
    }
    assert_eq!(seen.len(), 2, "both outcomes were reached: {seen:?}");
}

#[tokio::test]
async fn partitions_added_to_a_topic_are_given_to_a_member_that_subscribed_to_it() {
    use std::time::Duration;
    use support::{assigned_848, beat_848, join_848, rig_with};
    let rig = rig_with(|c| c.consumer_assignment_interval = Duration::ZERO);
    rig.create(vec![topic("t", 1)], false).await;
    let id = rig.metadata(12, Some(&["t"]), false).await.topics[0].topic_id;
    let joined = rig.heartbeat_848(1, join_848("g", "m1", &["t"])).await;
    let mut epoch = joined.member_epoch;
    let mut got = assigned_848(&joined);
    for _ in 0..3 {
        let next = rig.heartbeat_848(1, beat_848("g", "m1", epoch, None)).await;
        epoch = next.member_epoch;
        got = assigned_848(&next).or(got);
    }
    assert_eq!(got, Some(vec![(id, 0)]));
    assert_eq!(
        rig.create_partitions(3, false, &[("t", 3, None)]).await,
        [ok("t")]
    );
    for _ in 0..4 {
        let next = rig.heartbeat_848(1, beat_848("g", "m1", epoch, None)).await;
        epoch = next.member_epoch;
        got = assigned_848(&next).or(got);
    }
    assert_eq!(got, Some(vec![(id, 0), (id, 1), (id, 2)]));
}
