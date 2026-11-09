//! A stop at every point of CreatePartitions, by a kill and by a power cut: the topic has its old count or its new one,
//! and every partition it has is on the disk and works.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{open_on, records, rig, topic};

const INVALID_PARTITIONS: i16 = 37;

fn ok(name: &str) -> (String, i16, Option<String>) {
    (name.to_owned(), 0, None)
}

async fn partitions_of(rig: &support::Rig, name: &str) -> usize {
    rig.metadata(12, Some(&[name]), false).await.topics[0]
        .partitions
        .len()
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

/// A directory that cannot be made, after some were: the answer is the storage error, the topic keeps its count, and the
/// directories that were made for it are taken back (nothing is left on the disk that the topic does not have).
#[tokio::test]
async fn a_directory_that_cannot_be_made_leaves_the_topic_as_it_was_and_no_new_directory_behind() {
    use kafka_storage::sim::{Failure, Op};
    const KAFKA_STORAGE_ERROR: i16 = 56;
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let before = rig.dirs();
    assert_eq!(before, ["t-0", "t-1"]);
    // t-2 is made, t-3 is not.
    rig.fs.fail(Op::Mkdir, 2, Failure::Io);
    let got = rig.create_partitions(3, false, &[("t", 5, None)]).await;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1, KAFKA_STORAGE_ERROR, "{got:?}");
    assert_eq!(partitions_of(&rig, "t").await, 2);
    assert_eq!(rig.dirs(), before, "t-2 was taken back");
    // And the same request, asked again, works.
    assert_eq!(
        rig.create_partitions(3, false, &[("t", 5, None)]).await,
        [ok("t")]
    );
    assert_eq!(rig.dirs().len(), 5);
}
