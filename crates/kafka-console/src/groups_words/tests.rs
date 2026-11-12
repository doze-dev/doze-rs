//! The sentences of the Groups pages against the larder: what they say, and what they refuse to say.

use super::*;
use crate::fixtures_groups as world;
use crate::model::{GroupSpan, LagState};

fn flat(spans: &[GroupSpan]) -> String {
    spans
        .iter()
        .map(|s| match s {
            GroupSpan::Text(t)
            | GroupSpan::Mono(t)
            | GroupSpan::Strong(t)
            | GroupSpan::Topic(t) => t.as_str(),
        })
        .collect()
}

#[test]
fn a_stable_classic_group_is_told_by_its_member_its_topics_and_its_lag() {
    let g = world::group("search-indexer", false).unwrap();
    let s = flat(&group_sentence(&g.head, &g.topics));
    assert_eq!(
        s,
        "One member, search-indexer-1, reads clickstream (6 partitions) and inventory.levels (1). It is 18,240 records behind on clickstream; inventory.levels is caught up. Last commit 6 s ago."
    );
}

#[test]
fn a_consumer_group_in_step_says_nothing_is_moving_and_gives_no_cause() {
    let g = world::group("payments-workers", false).unwrap();
    let s = flat(&group_sentence(&g.head, &g.topics));
    assert!(
        s.starts_with("Three members hold orders (3 partitions)."),
        "{s}"
    );
    assert!(s.contains(
        "Every member holds exactly what the broker targeted for epoch 14, so nothing is moving."
    ));
    assert!(
        !s.contains("joined") && !s.contains("because"),
        "no cause: {s}"
    );
}

#[test]
fn a_rebalance_names_who_is_waited_for_and_the_time_left_and_never_a_cause() {
    let g = world::group("fulfilment", false).unwrap();
    let s = flat(&group_sentence(&g.head, &g.topics));
    assert!(s.starts_with("fulfilment-py-1 has not joined again, so the group is rebalancing. It is waiting for it: 2.1 s of the 5 s rebalance timeout are left."), "{s}");
    assert!(
        !s.contains("left 3 s ago"),
        "that needs the event ring (9f)"
    );
}

#[test]
fn the_lag_sentences_per_state() {
    let g = world::group("analytics", false).unwrap();
    assert_eq!(
        flat(&lag_sentence(
            &g.head,
            &g.topics,
            LagState::Ahead,
            g.retention_ms
        )),
        "analytics committed 16,200 on orders-0, but the log ends at 16,104: the commit is 96 past the end. Lag cannot be a negative number, so the console says where the commit is instead."
    );
    let e = world::group("analytics", true).unwrap();
    assert_eq!(
        flat(&lag_sentence(
            &e.head,
            &e.topics,
            LagState::Expiring,
            e.retention_ms
        )),
        "The broker forgets analytics' 3 committed offsets on Nov 13 00:14 UTC, in 3 d 2 h. The group has no members, and offsets are kept for 7 d."
    );
    let f = world::group("fulfilment", false).unwrap();
    let s = flat(&lag_sentence(
        &f.head,
        &f.topics,
        LagState::NoCommits,
        f.retention_ms,
    ));
    assert!(
        s.contains("pick-lists has no committed offset, so its lag is not known"),
        "{s}"
    );
}

#[test]
fn kafkas_descriptions_are_the_ones_the_boards_quote() {
    assert_eq!(
        description(25),
        "The coordinator is not aware of this member."
    );
    assert_eq!(description(68), "The group is not empty.");
    assert_eq!(
        description(86),
        "Deleting offsets of a topic is forbidden while the consumer group is actively subscribed to it."
    );
    let r = kafka_refusal(68);
    assert_eq!((r.code, r.name.as_str()), (68, "NON_EMPTY_GROUP"));
    assert_eq!(kafka_refusal(25).name, "UNKNOWN_MEMBER_ID");
}

#[test]
fn the_reading_of_a_refused_reset_counts_the_members_and_a_delete_names_the_rule() {
    let g = world::group("search-indexer", false).unwrap();
    let (rule, steps) = reset_reading(&g.head, 25);
    assert!(flat(&rule).contains("search-indexer has one, search-indexer-1"));
    assert_eq!(steps.len(), 3);
    assert!(
        steps[0].blocking
            && steps[0].value == "1 member now"
            && steps[0].sub.contains("45 s session")
    );
    let (rule, steps) = delete_reading(&g.head, 68);
    assert!(flat(&rule).contains("GROUP_ID_NOT_FOUND, 69"));
    assert_eq!(steps.len(), 1);
}

#[test]
fn times_and_numbers_read_as_a_person_says_them() {
    assert_eq!(span_precise(2_100), "2.1 s");
    assert_eq!(span_precise(5_000), "5 s");
    assert_eq!(seconds(60_000), "60 s");
    assert_eq!(seconds(180_000), "3 min");
    assert_eq!(number_word(3), "three");
    assert_eq!(number_word(11), "11");
    assert_eq!(members_word(1), "1 member");
}
