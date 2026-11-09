//! Consumer groups of the new protocol across the classic protocol and a restart: a classic member in a consumer group,
//! and an emptied consumer group kept when the process dies.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_wire::Uuid;
use support::{beat_848, join_848, rig_with, topic};

/// A rig whose assignments are computed at every heartbeat; topic `t` has three partitions.
async fn rig() -> (support::Rig, Uuid) {
    let rig = rig_with(|c| c.consumer_assignment_interval = Duration::ZERO);
    rig.create(vec![topic("t", 3)], false).await;
    let id = rig.metadata(12, Some(&["t"]), false).await.topics[0].topic_id;
    (rig, id)
}

#[tokio::test]
async fn a_classic_member_that_joins_a_consumer_group_can_commit_with_the_generation_it_was_given()
{
    let (rig, _) = rig().await;
    rig.heartbeat_848(1, join_848("g", "m1", &["t"])).await;
    let meta = support::subscription(&["t"]);
    let first = rig
        .join(4, support::join_request("g", "", None, &[("range", &meta)]))
        .await;
    assert_eq!(first.error_code, 79, "told to come back with an id");
    let id = first.member_id.to_string();
    let joined = rig
        .join(
            4,
            support::join_request("g", &id, None, &[("range", &meta)]),
        )
        .await;
    assert_eq!(joined.error_code, 0);
    let synced = rig.sync(4, "g", &id, joined.generation_id, &[]).await;
    assert_eq!(synced.error_code, 0);
    // The generation it was given is its epoch: a commit with it is taken, and until the group moves it again.
    let committed = rig
        .commit_offsets(8, "g", &id, joined.generation_id, &[("t", 0, 5, None)])
        .await;
    assert_eq!(
        committed.topics[0].partitions[0].error_code,
        0,
        "generation {} at epoch {:?}",
        joined.generation_id,
        rig.describe_848(1, &["g"]).await[0]
            .members
            .iter()
            .map(|m| (m.member_id.to_string(), m.member_epoch))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn an_emptied_consumer_group_is_still_one_after_a_restart_and_its_epochs_go_on() {
    let (rig, _) = rig().await;
    let joined = rig.heartbeat_848(1, join_848("g", "m1", &["t"])).await;
    rig.heartbeat_848(1, beat_848("g", "m1", -1, None)).await;
    let before = rig.describe_848(1, &["g"]).await.remove(0);
    assert_eq!(
        (before.group_state.as_str(), before.error_code),
        ("Empty", 0)
    );
    // The process dies: nothing of the group is in memory, only its record.
    let restarted = support::open_on_with(rig.fs.process_restart(), |c| {
        c.consumer_assignment_interval = Duration::ZERO;
    });
    let after = restarted.describe_848(1, &["g"]).await.remove(0);
    assert_eq!(
        (
            after.group_state.as_str(),
            after.group_epoch,
            after.assignment_epoch,
            after.assignor_name.as_str()
        ),
        (
            "Empty",
            before.group_epoch,
            before.assignment_epoch,
            "uniform"
        )
    );
    assert_eq!(
        restarted.list_groups_typed().await,
        [("g".to_owned(), "consumer".to_owned(), "Empty".to_owned())]
    );
    assert_eq!(
        restarted.describe_groups_state("g").await,
        "Dead",
        "the classic API does not describe it"
    );
    // A member joining it goes on from the epochs it had.
    let again = restarted
        .heartbeat_848(1, join_848("g", "m2", &["t"]))
        .await;
    assert!(
        again.member_epoch > joined.member_epoch,
        "{} after {}",
        again.member_epoch,
        joined.member_epoch
    );
}
