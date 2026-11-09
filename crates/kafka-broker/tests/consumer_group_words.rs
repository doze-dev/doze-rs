//! More of what a consumer group heartbeat is refused with, in Kafka's words (read from Apache Kafka 4.3.1's source: the
//! recordings hold the others, which are in `consumer_groups.rs`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{beat_848, join_848, join_request, rig, topic};

const INVALID_REQUEST: i16 = 42;
const GROUP_ID_NOT_FOUND: i16 = 69;

fn words(r: &kafka_wire::messages::ConsumerGroupHeartbeatResponse) -> (i16, String) {
    (
        r.error_code,
        r.error_message
            .as_ref()
            .map(|m| m.as_str().to_owned())
            .unwrap_or_default(),
    )
}

#[tokio::test]
async fn a_member_id_or_a_rack_that_is_blank_is_refused_and_a_first_request_needs_no_member_id_before_version_1()
 {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    let mut blank_rack = join_848("g", "m1", &["t"]);
    blank_rack.rack_id = Some(kafka_wire::StrBytes::from_string("  ".into()));
    assert_eq!(
        words(&rig.heartbeat_848(1, blank_rack).await),
        (INVALID_REQUEST, "RackId can't be empty.".into())
    );
    // From version 1 the client makes the member id; before, the broker does at the first heartbeat.
    assert_eq!(
        words(&rig.heartbeat_848(1, join_848("g", " ", &["t"])).await),
        (INVALID_REQUEST, "MemberId can't be empty.".into())
    );
    assert_eq!(
        rig.heartbeat_848(0, join_848("g", "", &["t"]))
            .await
            .error_code,
        0
    );
    // Keeping on or leaving names the member whatever the version.
    for epoch in [3, -1] {
        assert_eq!(
            words(&rig.heartbeat_848(0, beat_848("g", "", epoch, None)).await),
            (INVALID_REQUEST, "MemberId can't be empty.".into()),
            "epoch {epoch}"
        );
    }
}

#[tokio::test]
async fn a_group_of_classic_members_is_not_a_consumer_group_to_a_member_that_is_not_joining() {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    let first = rig
        .join(3, join_request("cg", "", None, &[("range", "m")]))
        .await;
    assert_eq!(first.error_code, 0);
    for epoch in [5, -1] {
        assert_eq!(
            words(
                &rig.heartbeat_848(1, beat_848("cg", "m1", epoch, None))
                    .await
            ),
            (
                GROUP_ID_NOT_FOUND,
                "Group cg is not a consumer group.".into()
            ),
            "epoch {epoch}"
        );
    }
    // Where there is no group at all it is the group of the new protocol that is not there.
    assert_eq!(
        words(
            &rig.heartbeat_848(1, beat_848("nobody", "m1", 5, None))
                .await
        ),
        (
            GROUP_ID_NOT_FOUND,
            "Consumer group nobody not found.".into()
        )
    );
}

#[tokio::test]
async fn a_commit_naming_no_generation_to_an_empty_consumer_group_is_accepted_whoever_it_says_it_is()
 {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    let joined = rig.heartbeat_848(1, join_848("cg", "m1", &["t"])).await;
    assert_eq!(joined.error_code, 0);
    assert_eq!(
        rig.heartbeat_848(1, beat_848("cg", "m1", -1, None))
            .await
            .error_code,
        0
    );
    // Nobody is in the group now. A commit with generation -1 (a tool resetting offsets, or a consumer not using the group)
    // is accepted, though it names a member; one that names a generation is not (Kafka's source).
    let ok = rig
        .commit_offsets(9, "cg", "someone", -1, &[("t", 0, 4, None)])
        .await;
    assert_eq!(ok.topics[0].partitions[0].error_code, 0);
    let named = rig
        .commit_offsets(9, "cg", "someone", 3, &[("t", 0, 5, None)])
        .await;
    assert_eq!(named.topics[0].partitions[0].error_code, 25);
}
