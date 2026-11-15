use kafka_broker::{DataUsage, Emptied, EmptiedAll, Erased, Keeps, Stray, TopicUsage};
use kafka_storage::fs::Space;

use super::*;

const GIB: u64 = 1 << 30;

fn keeps(compact: bool, delete: bool, age: Option<i64>, size: Option<u64>) -> Keeps {
    Keeps {
        compact,
        delete,
        max_age_ms: age,
        max_bytes: size,
        own: false,
    }
}

fn topic(name: &str, bytes: u64, k: Keeps) -> TopicUsage {
    TopicUsage {
        name: name.to_owned(),
        partitions: 3,
        bytes,
        segments: 3,
        oldest_ms: None,
        last_write_ms: None,
        keeps: k,
        next_drop_ms: None,
    }
}

fn usage(topics: Vec<TopicUsage>, space: Option<Space>) -> DataUsage {
    DataUsage {
        data_dir: "/data".into(),
        space,
        topics,
        hidden_bytes: 0,
        strays: Vec::new(),
        meta_bytes: 1 << 20,
        other_bytes: 0,
        other_files: 1,
        cut: false,
        groups: 2,
        transactional_ids: 1,
        defaults: keeps(false, true, None, None),
    }
}

#[test]
fn retention_is_said_short_as_the_table_shows_it() {
    assert_eq!(retention_short(&keeps(false, true, None, None)), "none");
    assert_eq!(
        retention_short(&keeps(true, false, None, None)),
        "latest per key"
    );
    assert_eq!(
        retention_short(&keeps(false, true, Some(604_800_000), None)),
        "7 days"
    );
    assert_eq!(
        retention_short(&keeps(false, true, None, Some(GIB))),
        "1.0 GiB per partition"
    );
    assert_eq!(
        retention_short(&keeps(true, true, Some(3_600_000), Some(GIB))),
        "1 hour or 1.0 GiB per partition"
    );
}

#[test]
fn a_nearly_full_disk_is_said_first_then_a_large_share_then_a_large_topic_that_keeps_everything() {
    let big = || vec![topic("orders", 3 * GIB, keeps(false, true, None, None))];
    let full = usage(
        big(),
        Some(Space {
            total: 500 * GIB,
            free: GIB,
        }),
    );
    let a = alarm(&full).unwrap();
    assert_eq!(
        (a.tone, a.headline.as_str()),
        (AlarmTone::Bad, "The disk is nearly full")
    );
    // 5% of the volume is the line too, on a big disk with more than 2 GiB free.
    let tight = usage(
        big(),
        Some(Space {
            total: 1000 * GIB,
            free: 40 * GIB,
        }),
    );
    assert_eq!(alarm(&tight).unwrap().tone, AlarmTone::Bad);
    let share = usage(
        big(),
        Some(Space {
            total: 20 * GIB,
            free: 10 * GIB,
        }),
    );
    let a = alarm(&share).unwrap();
    assert_eq!(
        (a.tone, a.headline.as_str()),
        (AlarmTone::Warn, "doze-kafka's data is 15% of the disk")
    );
    let roomy = usage(
        big(),
        Some(Space {
            total: 500 * GIB,
            free: 300 * GIB,
        }),
    );
    let a = alarm(&roomy).unwrap();
    assert_eq!(a.headline, "orders keeps everything: 3.0 GiB so far");
    assert_eq!(a.topic.as_deref(), Some("orders"));
    // Bounded, or small: nothing to say. A volume that does not say its size cannot be nearly full.
    let bounded = vec![topic("orders", 3 * GIB, keeps(false, true, Some(1), None))];
    assert_eq!(alarm(&usage(bounded, None)), None);
    let small = vec![topic("orders", GIB - 1, keeps(false, true, None, None))];
    assert_eq!(alarm(&usage(small, None)), None);
}

#[test]
fn the_kinds_are_largest_first_and_say_what_is_in_them() {
    let mut u = usage(vec![topic("a", 10, keeps(false, true, None, None))], None);
    u.strays.push(Stray {
        name: "ghost-0".into(),
        bytes: 5,
    });
    let rows = kinds(&u, Some(4));
    let order: Vec<DataKind> = rows.iter().map(|r| r.kind).collect();
    assert_eq!(
        order,
        [
            DataKind::Metadata,
            DataKind::Topics,
            DataKind::Strays,
            DataKind::Other
        ]
    );
    assert_eq!(
        rows[0].note,
        "meta.redb: the topics, the committed offsets of 2 groups, 1 transactional id, ACLs and users, 4 schema subjects."
    );
    assert_eq!(
        rows[1].note,
        "1 topic with 3 partitions: their segments and indexes."
    );
}

#[test]
fn what_each_action_did_is_one_sentence() {
    let one = Emptied {
        topic: "orders".into(),
        partitions: 3,
        offsets: 1234,
        freed_bytes: 13 << 20,
    };
    assert_eq!(
        emptied(&one),
        "Emptied orders: 1,234 records in 3 partitions, 13.0 MiB freed. It carries on from where it ended."
    );
    let none = Emptied {
        topic: "orders".into(),
        ..Emptied::default()
    };
    assert_eq!(emptied(&none), "orders was already empty: nothing to free.");
    let all = EmptiedAll {
        topics: vec![one.clone(), one],
    };
    assert_eq!(
        emptied_all(&all),
        "Emptied 2 topics: 2,468 records, 26.0 MiB freed. Every topic, partition and config is still there."
    );
    let e = Erased {
        topics: 5,
        freed_bytes: GIB,
        groups: 3,
        groups_kept: vec!["live".into()],
        transactional_ids: 1,
    };
    assert_eq!(
        erased(&e, Some(2)),
        "Erased 5 topics, 3 groups, 1 transactional id, 2 schema subjects; 1.0 GiB freed. ACLs, users and the broker's settings are as they were. The group live has members connected: it stays, with nothing committed."
    );
}
