//! A streams group across a stop (docs/port/behaviours/streams-groups.md, STR-80 to STR-82): its epochs and topology are
//! kept, written when a member is seated and when the last goes; its members are not (they join again, as after a
//! coordinator moves). The disk is the simulated one, the process killed (the disk keeps what was written) or the power cut
//! (it keeps only what was flushed), and also stopped after each operation of the first join in turn.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_storage::sim::{Cut, DataCut, NameCut, SimFs};
use support::streams::{heartbeat, join};
use support::{Rig, open_on, rig, topic};

const UNKNOWN_MEMBER_ID: i16 = 25;

async fn running() -> (Rig, i32) {
    let rig = rig();
    rig.create(vec![topic("in", 2)], false).await;
    let last = rig.streams_settle("app", "m", "in").await;
    (rig, last.member_epoch)
}

fn stops(rig: &Rig) -> [SimFs; 2] {
    [
        rig.fs.process_restart(),
        rig.fs.power_cut(Cut {
            data: DataCut::Drop,
            names: NameCut::Durable,
            seed: 7,
        }),
    ]
}

#[tokio::test]
async fn a_group_with_a_member_comes_back_empty_with_its_topology_and_its_member_joins_again_later() {
    let (rig, epoch) = running().await;
    for disk in stops(&rig) {
        let again = open_on(disk.process_restart());
        let d = again.streams_describe(&["app"]).await.groups.remove(0);
        assert_eq!((d.error_code, d.group_state.as_str()), (0, "Empty"), "{d:?}");
        let subs = d.topology.as_ref().unwrap().subtopologies.as_ref().unwrap();
        assert_eq!(subs.iter().map(|s| s.subtopology_id.to_string()).collect::<Vec<_>>(), ["0", "1"]);
        assert!(d.members.is_empty());
        // The member of the last run is not known; joining again, it is at a later epoch than the group had.
        let stale = again.streams_heartbeat(heartbeat("app", "m", epoch)).await;
        assert_eq!(stale.error_code, UNKNOWN_MEMBER_ID);
        let joined = again.streams_settle("app", "m", "in").await;
        assert!(joined.member_epoch > epoch, "{} after {epoch}", joined.member_epoch);
    }
}

#[tokio::test]
async fn an_empty_group_keeps_its_epochs() {
    let (rig, _) = running().await;
    let left = rig.streams_heartbeat(heartbeat("app", "m", -1)).await;
    assert_eq!(left.error_code, 0);
    let before = rig.streams_describe(&["app"]).await.groups.remove(0);
    for disk in stops(&rig) {
        let d = open_on(disk.process_restart()).streams_describe(&["app"]).await.groups.remove(0);
        assert_eq!((d.group_epoch, d.assignment_epoch), (before.group_epoch, before.assignment_epoch));
        assert_eq!(d.group_state.as_str(), "Empty");
    }
}

/// Stopped after each disk operation of the first join: the group is there with its topology, or not there at all; never
/// a group without its topology.
#[tokio::test]
async fn a_stop_at_any_point_of_the_first_join_leaves_the_group_whole_or_absent() {
    let clean = rig();
    clean.create(vec![topic("in", 2)], false).await;
    let before = clean.fs.ops();
    assert_eq!(clean.streams_heartbeat(join("app", "m", "in")).await.error_code, 0);
    let steps = clean.fs.ops() - before;
    assert!(steps >= 1, "{steps}");
    for stop in 0..=steps {
        let rig = rig();
        rig.create(vec![topic("in", 2)], false).await;
        rig.fs.halt_after(stop);
        let _ = rig.streams_heartbeat(join("app", "m", "in")).await;
        let disk = rig.fs.power_cut(Cut {
            data: DataCut::Drop,
            names: NameCut::Durable,
            seed: stop,
        });
        let d = open_on(disk.process_restart()).streams_describe(&["app"]).await.groups.remove(0);
        if d.error_code == 0 {
            assert!(d.topology.is_some(), "stop {stop}: {d:?}");
        } else {
            assert_eq!(d.error_code, 69, "stop {stop}: {d:?}");
        }
    }
}
