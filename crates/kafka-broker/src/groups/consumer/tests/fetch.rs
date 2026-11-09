//! Who may fetch a consumer group's offsets.

use kafka_wire::ResponseError as E;
use kafka_wire::ids::MemberId;

use super::*;

fn view() -> ConsumerView {
    let member = |id: &str, epoch, classic| MemberView {
        id: MemberId::new(id).unwrap(),
        instance: None,
        rack: None,
        client_id: String::new(),
        client_host: "/127.0.0.1".into(),
        epoch,
        subscribed: vec![],
        regex: String::new(),
        assigned: Partitions::new(),
        target: Partitions::new(),
        classic,
    };
    ConsumerView {
        state: "Stable",
        epoch: 5,
        assignment_epoch: 5,
        assignor: "uniform",
        members: vec![member("new", 5, false), member("old", 4, true)],
    }
}

#[test]
fn a_member_is_known_by_its_id_and_its_epoch() {
    let v = view();
    assert_eq!(fetch_refusal(&v, Some("new"), 5), None);
    assert_eq!(fetch_refusal(&v, Some("new"), 4), Some(E::StaleMemberEpoch));
    assert_eq!(
        fetch_refusal(&v, Some("new"), -1),
        Some(E::StaleMemberEpoch)
    );
    assert_eq!(
        fetch_refusal(&v, Some("ghost"), 5),
        Some(E::UnknownMemberId)
    );
}

#[test]
fn a_classic_member_of_the_group_is_told_its_generation_is_wrong() {
    let v = view();
    assert_eq!(fetch_refusal(&v, Some("old"), 4), None);
    assert_eq!(
        fetch_refusal(&v, Some("old"), 3),
        Some(E::IllegalGeneration)
    );
}

#[test]
fn a_request_naming_no_member_and_no_epoch_is_not_checked() {
    let v = view();
    assert_eq!(fetch_refusal(&v, None, -1), None);
    // An epoch with no member is a member that is not there.
    assert_eq!(fetch_refusal(&v, None, 5), Some(E::UnknownMemberId));
    // A name with no epoch is checked too: the epoch it is at is not -1.
    assert_eq!(fetch_refusal(&v, Some(""), -1), Some(E::UnknownMemberId));
}
