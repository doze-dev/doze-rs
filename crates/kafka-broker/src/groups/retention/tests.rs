//! The rules for which offsets expire.

use kafka_meta::StoredOffset;

use super::*;

const DAY: i64 = 86_400_000;

fn offset(topic: &str, partition: i32, committed_ms: i64) -> GroupOffset {
    GroupOffset {
        topic: TopicName::parse(topic).unwrap(),
        partition: PartitionId::new(partition).unwrap(),
        offset: StoredOffset {
            offset: 1,
            leader_epoch: -1,
            metadata: String::new(),
            committed_ms,
        },
    }
}

fn life(live: bool, session_ms: u32, empty_since_ms: Option<i64>) -> GroupLife {
    GroupLife {
        live,
        session_ms,
        empty_since_ms,
        protocol_type: "consumer".into(),
        consumer: None,
    }
}

fn names(list: Vec<Expired>) -> Vec<String> {
    let mut out: Vec<_> = list.iter().map(|(t, p, _)| format!("{t}-{p}")).collect();
    out.sort();
    out
}

/// A week's retention; now is day 10; the broker started at day 9.
fn run(view: &View<'_>) -> Vec<String> {
    names(expired(view, 10 * DAY, 7 * DAY, 9 * DAY))
}

#[test]
fn a_group_that_never_had_a_member_keeps_each_offset_for_the_retention_from_its_own_commit() {
    let offsets = [offset("t", 0, 2 * DAY), offset("t", 1, 4 * DAY)];
    for members in [Members::Empty, Members::NotInMemory] {
        let view = View {
            members,
            life: None,
            offsets: &offsets,
        };
        assert_eq!(run(&view), ["t-0"], "day 2 is eight days old, day 4 is six");
    }
}

#[test]
fn an_emptied_group_keeps_its_offsets_for_the_retention_from_when_it_became_empty_however_late_the_commit()
 {
    // Empty since day 5 (five days ago): kept, even though the offset is eight days old.
    let offsets = [offset("t", 0, 2 * DAY)];
    let l = life(false, 10_000, Some(5 * DAY));
    let view = View {
        members: Members::Empty,
        life: Some(&l),
        offsets: &offsets,
    };
    assert!(run(&view).is_empty());
    // Empty since day 2: all go, even an offset committed yesterday (a commit into an empty group does not extend).
    let offsets = [offset("t", 0, 2 * DAY), offset("t", 1, 9 * DAY)];
    let l = life(false, 10_000, Some(2 * DAY));
    let view = View {
        members: Members::Empty,
        life: Some(&l),
        offsets: &offsets,
    };
    assert_eq!(run(&view), ["t-0", "t-1"]);
}

#[test]
fn a_group_with_members_expires_nothing_for_the_topics_it_subscribes_to_and_the_rest_by_their_own_age()
 {
    let subscribed: BTreeSet<String> = ["t".to_owned()].into();
    let offsets = [
        offset("t", 0, DAY),
        offset("u", 0, DAY),
        offset("u", 1, 9 * DAY),
    ];
    let view = View {
        members: Members::Running {
            rebalancing: false,
            consumer: true,
            subscribed: Some(&subscribed),
        },
        life: None,
        offsets: &offsets,
    };
    assert_eq!(run(&view), ["u-0"]);
}

#[test]
fn a_group_in_a_rebalance_or_of_another_kind_or_with_an_unreadable_subscription_is_not_looked_at() {
    let subscribed: BTreeSet<String> = BTreeSet::new();
    let offsets = [offset("u", 0, DAY)];
    let looked = |rebalancing, consumer, subscribed: Option<&BTreeSet<String>>| {
        run(&View {
            members: Members::Running {
                rebalancing,
                consumer,
                subscribed,
            },
            life: None,
            offsets: &offsets,
        })
    };
    assert_eq!(
        looked(false, true, Some(&subscribed)),
        ["u-0"],
        "the control: looked at"
    );
    assert!(looked(true, true, Some(&subscribed)).is_empty());
    assert!(looked(false, false, Some(&subscribed)).is_empty());
    assert!(looked(false, true, None).is_empty());
}

#[test]
fn a_group_recorded_as_having_members_that_nobody_has_rejoined_is_presumed_to_have_them_until_their_session_ends()
 {
    let offsets = [offset("t", 0, DAY)];
    let ran = |started_ms: i64, session_ms: u32| {
        let l = life(true, session_ms, None);
        let view = View {
            members: Members::NotInMemory,
            life: Some(&l),
            offsets: &offsets,
        };
        names(expired(&view, 10 * DAY, 7 * DAY, started_ms))
    };
    // Started yesterday: the members are taken to have left only a minute after that, which is not a week ago.
    assert!(ran(9 * DAY, 60_000).is_empty());
    // Started eight days ago: gone from a minute after that, which is more than a week ago.
    assert_eq!(ran(2 * DAY, 60_000), ["t-0"]);
    // The same start with a session of nearly fifty days: still presumed to be there.
    assert!(ran(2 * DAY, u32::MAX).is_empty());
}

#[test]
fn a_record_that_says_members_for_a_group_in_memory_without_any_keeps_the_offsets() {
    let offsets = [offset("t", 0, DAY)];
    let l = life(true, 10_000, None);
    let view = View {
        members: Members::Empty,
        life: Some(&l),
        offsets: &offsets,
    };
    assert!(run(&view).is_empty(), "unknown keeps");
}

#[test]
fn a_retention_of_zero_keeps_everything() {
    let offsets = [offset("t", 0, 0)];
    let view = View {
        members: Members::Empty,
        life: None,
        offsets: &offsets,
    };
    assert!(expired(&view, 10 * DAY, 0, 9 * DAY).is_empty());
}

fn consumer_life() -> kafka_meta::ConsumerLife {
    kafka_meta::ConsumerLife {
        epoch: 3,
        assignment_epoch: 2,
        assignor: "uniform".into(),
    }
}

#[test]
fn an_emptied_consumer_group_keeps_each_offset_for_the_retention_from_its_own_commit() {
    // Empty since day 5, but a consumer group has no time of becoming empty: day 2 is eight days old, day 4 is six.
    let offsets = [offset("t", 0, 2 * DAY), offset("t", 1, 4 * DAY)];
    let mut l = life(false, 0, None);
    l.consumer = Some(consumer_life());
    let view = View {
        members: Members::Empty,
        life: Some(&l),
        offsets: &offsets,
    };
    assert_eq!(run(&view), ["t-0"]);
}

#[test]
fn a_consumer_group_recorded_as_having_members_is_waited_for_until_their_session_has_run_out() {
    let offsets = [offset("t", 0, 2 * DAY)];
    let mut l = life(true, 3 * DAY as u32, None);
    l.consumer = Some(consumer_life());
    // Started at day 9 with a session of three days: they may still return until day 12; now is day 10.
    let view = View {
        members: Members::NotInMemory,
        life: Some(&l),
        offsets: &offsets,
    };
    assert!(run(&view).is_empty());
    // With a short session they are gone, and the old offset goes, counted from its own commit.
    l.session_ms = 10_000;
    let view = View {
        members: Members::NotInMemory,
        life: Some(&l),
        offsets: &offsets,
    };
    assert_eq!(run(&view), ["t-0"]);
}
