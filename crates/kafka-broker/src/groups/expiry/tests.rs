//! The date the console shows is the date the sweep acts on: `first_expiry` against `expired` itself.

use std::collections::BTreeSet;

use kafka_meta::{ConsumerLife, GroupLife, StoredOffset};
use kafka_wire::ids::{PartitionId, TopicName};

use super::*;
use crate::groups::expired;

const DAY: i64 = 86_400_000;
const RETENTION: i64 = 7 * DAY;
const STARTED: i64 = 9 * DAY;

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

fn life(live: bool, empty_since_ms: Option<i64>, consumer: bool) -> GroupLife {
    GroupLife {
        live,
        session_ms: 45_000,
        empty_since_ms,
        protocol_type: "consumer".into(),
        consumer: consumer.then(|| ConsumerLife {
            epoch: 3,
            assignment_epoch: 3,
            assignor: "uniform".into(),
        }),
    }
}

/// Whether `expired` takes the offset at `index` when it is `now`.
fn taken(view: &View<'_>, index: usize, now: i64) -> bool {
    let o = &view.offsets[index];
    expired(view, now, RETENTION, STARTED)
        .iter()
        .any(|(t, p, _)| *t == o.topic && *p == o.partition)
}

/// The answer agrees with `expired` at the moment before it, at it and after it; `None` means never, over a long span.
fn agrees(view: &View<'_>) {
    let firsts = first_expiry(view, RETENTION, STARTED);
    assert_eq!(firsts.len(), view.offsets.len());
    for (i, first) in firsts.iter().enumerate() {
        match first {
            Some(f) => {
                assert!(!taken(view, i, f - 1), "taken before its date {f}");
                assert!(taken(view, i, *f), "not taken at its date {f}");
                assert!(taken(view, i, f + DAY), "not taken after its date {f}");
            }
            None => {
                for day in 0..60 {
                    assert!(!taken(view, i, day * DAY), "taken on day {day}");
                }
            }
        }
    }
}

#[test]
fn the_date_is_the_one_the_sweep_acts_on_over_a_grid_of_groups() {
    let subscribed: BTreeSet<String> = ["orders".to_owned()].into();
    let offsets = [
        offset("orders", 0, 2 * DAY),
        offset("orders", 1, 5 * DAY),
        offset("clicks", 0, 3 * DAY),
        offset("clicks", 1, 9 * DAY),
    ];
    let lives = [
        None,
        Some(life(true, None, false)),
        Some(life(false, Some(4 * DAY), false)),
        Some(life(false, None, false)),
        Some(life(true, None, true)),
        Some(life(false, Some(4 * DAY), true)),
    ];
    let mut checked = 0;
    for l in &lives {
        let members = [
            Members::Empty,
            Members::NotInMemory,
            Members::Running {
                rebalancing: false,
                consumer: true,
                subscribed: Some(&subscribed),
            },
            Members::Running {
                rebalancing: true,
                consumer: true,
                subscribed: Some(&subscribed),
            },
            Members::Running {
                rebalancing: false,
                consumer: false,
                subscribed: None,
            },
        ];
        for m in members {
            let view = View {
                members: m,
                life: l.as_ref(),
                offsets: &offsets,
            };
            agrees(&view);
            checked += 1;
        }
    }
    assert_eq!(checked, 30);
}

#[test]
fn an_emptied_group_is_forgotten_a_retention_after_it_emptied_not_after_the_last_commit() {
    let l = life(false, Some(5 * DAY), false);
    let offsets = [offset("orders", 0, 8 * DAY)];
    let view = View {
        members: Members::Empty,
        life: Some(&l),
        offsets: &offsets,
    };
    assert_eq!(
        first_expiry(&view, RETENTION, STARTED),
        vec![Some(12 * DAY + 1)]
    );
}

#[test]
fn nothing_is_dated_with_no_retention_or_with_a_subscribed_topic() {
    let subscribed: BTreeSet<String> = ["orders".to_owned()].into();
    let offsets = [offset("orders", 0, 0), offset("clicks", 0, 0)];
    let view = View {
        members: Members::Running {
            rebalancing: false,
            consumer: true,
            subscribed: Some(&subscribed),
        },
        life: None,
        offsets: &offsets,
    };
    assert_eq!(
        first_expiry(&view, RETENTION, STARTED),
        vec![None, Some(7 * DAY + 1)]
    );
    assert_eq!(first_expiry(&view, 0, STARTED), vec![None, None]);
}
