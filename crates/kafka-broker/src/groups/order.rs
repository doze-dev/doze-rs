//! The order Kafka answers a request that names several groups in.
//!
//! The same for every order of the request. Kafka's coordinator gathers the groups by the partition of
//! `__consumer_offsets` each belongs to, in a hash map keyed by that partition, and answers partition by partition as
//! the map iterates: Java's `HashMap`, its buckets in ascending order, the partitions that share a bucket in the
//! reverse of the order they were first named, and each partition's groups in the request's order. A client that
//! pairs answers with its request by position rather than by the group id each carries sees the same pairing here
//! as on Kafka. Recorded for up to 20 groups over 14 partitions, which takes the map past one resize
//! (`describe-groups-order`).

use std::collections::HashMap;

/// `offsets.topic.num.partitions` at Kafka's default, which the order is a function of.
const OFFSETS_PARTITIONS: i32 = 50;

/// Java's `String.hashCode`: over UTF-16 code units.
fn java_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0_i32, |h, u| h.wrapping_mul(31).wrapping_add(i32::from(u)))
}

/// Kafka's `partitionFor(groupId)`: `Utils.abs(hashCode) % 50`, where the absolute value of the least int is 0.
fn offsets_partition(group: &str) -> i32 {
    let h = java_hash(group);
    if h == i32::MIN {
        0
    } else {
        h.abs() % OFFSETS_PARTITIONS
    }
}

/// Java's `TopicPartition.hashCode` for `__consumer_offsets`.
fn topic_partition_hash(partition: i32) -> i32 {
    let result = 31_i32.wrapping_add(partition);
    result
        .wrapping_mul(31)
        .wrapping_add(java_hash("__consumer_offsets"))
}

/// `groups` (duplicates kept) in the order Kafka answers them.
#[must_use]
pub(crate) fn kafka_order<S: AsRef<str> + Clone>(groups: &[S]) -> Vec<S> {
    let mut parts: Vec<i32> = Vec::new(); // in the order first named
    let mut by_part: HashMap<i32, Vec<&S>> = HashMap::new();
    for g in groups {
        let p = offsets_partition(g.as_ref());
        if !by_part.contains_key(&p) {
            parts.push(p);
        }
        by_part.entry(p).or_default().push(g);
    }
    // A HashMap's default capacity is 16, doubled each time it is more than three quarters full.
    let mut capacity = 16_usize;
    while parts.len() > capacity * 3 / 4 {
        capacity *= 2;
    }
    let bucket = |p: i32| {
        let h = u32::from_ne_bytes(topic_partition_hash(p).to_ne_bytes());
        usize::try_from((h ^ (h >> 16)) & u32::try_from(capacity - 1).unwrap_or(u32::MAX))
            .unwrap_or(0)
    };
    let mut out = Vec::with_capacity(groups.len());
    for b in 0..capacity {
        // A bucket's latest partition comes first.
        for p in parts.iter().rev().filter(|p| bucket(**p) == b) {
            if let Some(names) = by_part.get(p) {
                out.extend(names.iter().map(|g| (*g).clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// Kafka's own answers, recorded: (the request, the order it was answered in).
    const RECORDED: &[(&[&str], &[&str])] = &[
        (
            &["dgo-a", "dgo-b", "dgo-c", "dgo-d", "dgo-x", "dgo-y"],
            &["dgo-x", "dgo-y", "dgo-d", "dgo-b", "dgo-c", "dgo-a"],
        ),
        (
            &["dgo-y", "dgo-x", "dgo-d", "dgo-c", "dgo-b", "dgo-a"],
            &["dgo-x", "dgo-y", "dgo-d", "dgo-b", "dgo-c", "dgo-a"],
        ),
        (&["dgo-b", "dgo-a", "dgo-b"], &["dgo-b", "dgo-b", "dgo-a"]),
        (&["dgo-a", "dgo-d"], &["dgo-d", "dgo-a"]),
        (
            &["dgo-c", "", "dgo-x", "dgo-a"],
            &["", "dgo-x", "dgo-c", "dgo-a"],
        ),
        (&["dgo-n3", "dgo-y"], &["dgo-y", "dgo-n3"]),
        (&["dgo-y", "dgo-n3"], &["dgo-n3", "dgo-y"]),
        (&["dgo-n3", "dgo-y", "dgo-x"], &["dgo-x", "dgo-y", "dgo-n3"]),
        (
            &["dgo-n1", "dgo-n3", "dgo-y"],
            &["dgo-n1", "dgo-y", "dgo-n3"],
        ),
        (
            &["dgo-y", "dgo-n1", "dgo-n3"],
            &["dgo-n1", "dgo-n3", "dgo-y"],
        ),
        (
            &["dgo-p8", "dgo-x", "dgo-p44"],
            &["dgo-p8", "dgo-x", "dgo-p44"],
        ),
        (
            &["dgo-p44", "dgo-x", "dgo-p8"],
            &["dgo-p44", "dgo-x", "dgo-p8"],
        ),
        (
            &[
                "dgo-p0", "dgo-p1", "dgo-p2", "dgo-p3", "dgo-p4", "dgo-p5", "dgo-p6", "dgo-p7",
                "dgo-p8", "dgo-p9", "dgo-p10", "dgo-p11", "dgo-p12", "dgo-p13", "dgo-p14",
                "dgo-p15", "dgo-p16", "dgo-p17", "dgo-p18", "dgo-p19",
            ],
            &[
                "dgo-p2", "dgo-p13", "dgo-p1", "dgo-p14", "dgo-p4", "dgo-p11", "dgo-p3", "dgo-p12",
                "dgo-p6", "dgo-p5", "dgo-p10", "dgo-p8", "dgo-p7", "dgo-p19", "dgo-p17", "dgo-p18",
                "dgo-p0", "dgo-p15", "dgo-p16", "dgo-p9",
            ],
        ),
        (
            &[
                "dgo-p19", "dgo-p18", "dgo-p17", "dgo-p16", "dgo-p15", "dgo-p14", "dgo-p13",
                "dgo-p12", "dgo-p11", "dgo-p10", "dgo-p9", "dgo-p8", "dgo-p7", "dgo-p6", "dgo-p5",
                "dgo-p4", "dgo-p3", "dgo-p2", "dgo-p1", "dgo-p0",
            ],
            &[
                "dgo-p13", "dgo-p2", "dgo-p14", "dgo-p1", "dgo-p11", "dgo-p4", "dgo-p12", "dgo-p3",
                "dgo-p6", "dgo-p10", "dgo-p5", "dgo-p8", "dgo-p7", "dgo-p19", "dgo-p17", "dgo-p18",
                "dgo-p15", "dgo-p0", "dgo-p16", "dgo-p9",
            ],
        ),
    ];

    #[test]
    fn groups_come_in_the_order_apache_kafka_answered_them_including_past_a_resize_of_its_map() {
        for (asked, answered) in RECORDED {
            assert_eq!(kafka_order(&names(asked)), names(answered), "{asked:?}");
        }
    }

    #[test]
    fn javas_string_hash_is_over_utf16_units_and_wraps() {
        assert_eq!(java_hash("a"), 97);
        assert_eq!(java_hash("ab"), 97 * 31 + 98);
        assert_eq!(
            java_hash("polygenelubricants"),
            i32::MIN,
            "the classic example"
        );
        assert_eq!(
            offsets_partition("polygenelubricants"),
            0,
            "abs of the least int is 0"
        );
        assert_eq!(
            java_hash("😀"),
            0xD83D * 31 + 0xDE00,
            "a surrogate pair is two units"
        );
    }
}
