//! A share group's records across a stop (docs/port/behaviours/share-groups.md, SG-50 to SG-53): what was acknowledged or
//! archived is never delivered again, what was released comes back with the deliveries it had, and what was only acquired
//! comes back as if it had not been (Kafka writes no acquisition: a lock does not outlive the broker, at-least-once). The
//! disk is the simulated one, stopped after each operation of an acknowledgement in turn, then the process killed (the disk
//! keeps what was written) or the power cut (it keeps only what was flushed).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_storage::sim::{Cut, DataCut, NameCut, SimFs};
use kafka_wire::Uuid;
use support::share::{acquired, ack_request, fetch_request};
use support::{Rig, open_on, records, rig, topic};

const ACCEPT: i8 = 1;
const RELEASE: i8 = 2;
const REJECT: i8 = 3;

/// Six records in `t`, a member of `g` (from the earliest) holding all six.
async fn holding() -> (Rig, Uuid) {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let t = rig.metadata(12, Some(&["t"]), false).await.topics[0].topic_id;
    assert_eq!(rig.group_config("g", &[("share.auto.offset.reset", "earliest")]).await, 0);
    rig.produce(-1, "t", 0, records(6)).await;
    assert_eq!(rig.share_join("g", "m", &["t"]).await.error_code, 0);
    let got = rig.share_fetch(fetch_request("g", "m", 0, &[(t, 0, vec![])])).await;
    assert_eq!(acquired(&got), [(0, 0, 5, 1)]);
    (rig, t)
}

/// 0 accepted, 1 released, 2 rejected; 3 to 5 still held. The partition's acknowledgement code.
async fn acknowledge(rig: &Rig, t: Uuid) -> i16 {
    let r = rig
        .share_ack_at(1, ack_request("g", "m", 1, &[(t, 0, vec![(0, 2, vec![ACCEPT, RELEASE, REJECT])])]))
        .await;
    assert_eq!(r.error_code, 0);
    r.responses[0].partitions[0].error_code
}

/// What a member that joins after the restart is given first.
async fn first_after(disk: SimFs, t: Uuid) -> Vec<(i32, i64, i64, i16)> {
    let rig = open_on(disk);
    assert_eq!(rig.share_join("g", "n", &["t"]).await.error_code, 0);
    acquired(&rig.share_fetch(fetch_request("g", "n", 0, &[(t, 0, vec![])])).await)
}

/// Nothing stored: all six again, first delivery (what was acquired is not counted).
const BEFORE: [(i32, i64, i64, i16); 1] = [(0, 0, 5, 1)];
/// The acknowledgement stored: 0 and 2 never again, 1 on its second delivery, 3 to 5 as never delivered.
const AFTER: [(i32, i64, i64, i16); 2] = [(0, 1, 1, 2), (0, 3, 5, 1)];

#[tokio::test]
async fn an_acknowledgement_survives_a_restart_and_a_lock_does_not() {
    let (rig, t) = holding().await;
    assert_eq!(acknowledge(&rig, t).await, 0);
    for disk in [
        rig.fs.process_restart(),
        rig.fs.power_cut(Cut {
            data: DataCut::Drop,
            names: NameCut::Durable,
            seed: 1,
        }),
    ] {
        assert_eq!(first_after(disk.process_restart(), t).await, AFTER);
    }
}

#[tokio::test]
async fn a_stop_at_any_point_of_an_acknowledgement_leaves_it_done_or_not_and_done_if_it_was_answered() {
    let (clean, t) = holding().await;
    let before = clean.fs.ops();
    assert_eq!(acknowledge(&clean, t).await, 0);
    let steps = clean.fs.ops() - before;
    assert!(steps >= 2, "{steps}");
    for stop in 0..=steps {
        for power in [false, true] {
            let (rig, t) = holding().await;
            rig.fs.halt_after(stop);
            let answer = acknowledge(&rig, t).await;
            let disk = if power {
                rig.fs.power_cut(Cut {
                    data: DataCut::Drop,
                    names: NameCut::Durable,
                    seed: stop,
                })
            } else {
                rig.fs.process_restart()
            };
            let after = first_after(disk.process_restart(), t).await;
            let what = format!("stop {stop}/{steps}, power {power}, answered {answer}");
            assert!(after == BEFORE || after == AFTER, "{what}: {after:?}");
            if answer == 0 {
                assert_eq!(after, AFTER, "{what}: acknowledged, so stored");
            }
        }
    }
}

#[tokio::test]
async fn a_failed_write_answers_the_acknowledgement_with_an_error_and_changes_nothing() {
    let (rig, t) = holding().await;
    rig.fs.halt_after(0);
    let answer = acknowledge(&rig, t).await;
    assert_ne!(answer, 0, "the write could not be made");
    // In memory nothing moved: 0 to 5 are still held, so another member gets nothing now.
    let other = rig.share_fetch(fetch_request("g", "x", 0, &[(t, 0, vec![])])).await;
    assert!(acquired(&other).is_empty(), "{:?}", acquired(&other));
}

#[tokio::test]
async fn a_share_groups_epochs_and_configs_outlive_a_restart() {
    let (rig, _) = holding().await;
    assert_eq!(rig.share_heartbeat(support::share::heartbeat_request("g", "m", -1, None)).await.error_code, 0);
    let again = open_on(rig.fs.process_restart());
    let d = again.share_describe(&["g"]).await;
    assert_eq!((d.groups[0].error_code, d.groups[0].group_state.as_str()), (0, "Empty"));
    assert_eq!((d.groups[0].group_epoch, d.groups[0].assignment_epoch), (2, 2));
    let listed = again.list_groups(&[], &["share"]).await;
    assert_eq!(listed, [("g".to_owned(), "Empty".to_owned(), "share".to_owned())]);
    let c = again.describe_configs(4, 32, "g", Some(&["share.auto.offset.reset"]), false, false).await;
    assert_eq!(c.configs[0].value.as_deref(), Some("earliest"));
}
