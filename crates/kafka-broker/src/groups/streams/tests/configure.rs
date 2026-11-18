//! STR-20 to STR-27: a topology against the topics there are (Kafka's `InternalTopicManager`).

use super::super::topology::{
    Copartition, INCORRECTLY_PARTITIONED_TOPICS, MISSING_INTERNAL_TOPICS, MISSING_SOURCE_TOPICS,
    Subtopology, Topology, configure, summarize,
};
use super::{internal, ready_topics, topics, word_count};

fn sub(sources: &[&str]) -> Subtopology {
    Subtopology {
        sources: sources.iter().map(|s| (*s).to_owned()).collect(),
        ..Subtopology::default()
    }
}

#[test]
fn missing_source_topics_are_named_sorted_three_at_most() {
    let mut t = Topology::default();
    t.subtopologies
        .insert("0".into(), sub(&["e", "d", "c", "b", "a"]));
    let c = configure(&t, &topics(&[])).unwrap();
    let n = c.not_ready.unwrap();
    assert_eq!(n.code, MISSING_SOURCE_TOPICS);
    assert_eq!(
        n.detail,
        "Source topics a, b, c and 2 additional topics are missing."
    );
    assert!(c.subtopologies.is_empty() && c.create.is_empty());
    assert_eq!(summarize(["x"]), "x");
    assert_eq!(summarize(Vec::<&str>::new()), "<none>");
}

#[test]
fn internal_topics_take_the_partitions_of_what_feeds_them_and_are_to_be_made() {
    let c = configure(&word_count(), &topics(&[("in", 4)])).unwrap();
    let n = c.not_ready.clone().unwrap();
    assert_eq!(n.code, MISSING_INTERNAL_TOPICS);
    assert_eq!(
        n.detail,
        "Internal topics are missing: app-counts-changelog, app-rep"
    );
    let made: Vec<(&str, i32, i16)> = c
        .create
        .iter()
        .map(|t| (t.name.as_str(), t.partitions, t.replication))
        .collect();
    assert_eq!(made, [("app-counts-changelog", 4, -1), ("app-rep", 4, -1)]);
    // The subtopologies are worked out even so: four tasks each.
    assert_eq!(c.subtopologies["0"].tasks, 4);
    assert_eq!(c.subtopologies["1"].tasks, 4);
    assert!(c.subtopologies["1"].stateful && !c.subtopologies["0"].stateful);
}

#[test]
fn a_topology_whose_topics_all_exist_is_ready_and_makes_nothing() {
    let c = configure(&word_count(), &ready_topics(3)).unwrap();
    assert!(c.ready(), "{:?}", c.not_ready);
    assert!(c.create.is_empty());
}

#[test]
fn an_internal_topic_that_exists_with_other_partitions_is_named_and_nothing_is_made() {
    let c = configure(&word_count(), &topics(&[("in", 3), ("app-rep", 2)])).unwrap();
    let n = c.not_ready.unwrap();
    assert_eq!(n.code, INCORRECTLY_PARTITIONED_TOPICS);
    assert_eq!(
        n.detail,
        "Existing topic app-rep has different number of partitions: expected 3, found 2"
    );
    assert!(c.create.is_empty() && c.subtopologies.is_empty());
}

#[test]
fn a_repartition_topic_nothing_writes_is_an_invalid_topology() {
    let mut t = Topology::default();
    t.subtopologies.insert(
        "0".into(),
        Subtopology {
            repartition_sources: vec![internal("orphan", 0)],
            ..Subtopology::default()
        },
    );
    let e = configure(&t, &topics(&[])).unwrap_err();
    assert!(!e.internal);
    assert_eq!(
        e.words,
        "Failed to compute number of partitions for all repartition topics, because a repartition source topic is never used as a sink topic."
    );
}

#[test]
fn repartition_topics_that_only_feed_each_other_are_a_loop() {
    let mut t = Topology::default();
    for (id, from, to) in [("0", "a", "b"), ("1", "b", "a")] {
        t.subtopologies.insert(
            id.into(),
            Subtopology {
                repartition_sources: vec![internal(from, 0)],
                repartition_sinks: vec![to.into()],
                ..Subtopology::default()
            },
        );
    }
    let e = configure(&t, &topics(&[])).unwrap_err();
    assert!(
        e.words.contains("There may be loops in the topology"),
        "{}",
        e.words
    );
}

#[test]
fn a_fixed_repartition_count_is_kept_and_counts_for_tasks() {
    let mut t = word_count();
    if let Some(s) = t.subtopologies.get_mut("1") {
        s.repartition_sources = vec![internal("app-rep", 6)];
    }
    let c = configure(&t, &topics(&[("in", 2)])).unwrap();
    let rep = c.create.iter().find(|t| t.name == "app-rep").unwrap();
    assert_eq!(rep.partitions, 6);
    assert_eq!(c.subtopologies["1"].tasks, 6);
    // The changelog follows what its subtopology reads.
    assert_eq!(
        c.create
            .iter()
            .find(|t| t.name == "app-counts-changelog")
            .unwrap()
            .partitions,
        6
    );
}

#[test]
fn copartitioned_sources_must_have_the_same_partitions() {
    let mut t = Topology::default();
    t.subtopologies.insert(
        "0".into(),
        Subtopology {
            sources: vec!["left".into(), "right".into()],
            copartition: vec![Copartition {
                sources: vec![0, 1],
                repartition_sources: vec![],
            }],
            ..Subtopology::default()
        },
    );
    let c = configure(&t, &topics(&[("left", 2), ("right", 3)])).unwrap();
    let n = c.not_ready.unwrap();
    assert_eq!(n.code, INCORRECTLY_PARTITIONED_TOPICS);
    assert_eq!(
        n.detail,
        "Following topics do not have the same number of partitions: [{left=2, right=3}]"
    );
    let ok = configure(&t, &topics(&[("left", 3), ("right", 3)])).unwrap();
    assert!(ok.ready());
    assert_eq!(ok.subtopologies["0"].tasks, 3);
}

#[test]
fn a_flexible_repartition_topic_copartitioned_with_a_source_takes_its_count() {
    let mut t = Topology::default();
    t.subtopologies.insert(
        "0".into(),
        Subtopology {
            sources: vec!["in".into()],
            repartition_sinks: vec!["rep".into()],
            ..Subtopology::default()
        },
    );
    t.subtopologies.insert(
        "1".into(),
        Subtopology {
            sources: vec!["table".into()],
            repartition_sources: vec![internal("rep", 0)],
            copartition: vec![Copartition {
                sources: vec![0],
                repartition_sources: vec![0],
            }],
            ..Subtopology::default()
        },
    );
    // The sink is worked out from `in` (5); copartitioning with `table` (2) sets it to 2.
    let c = configure(&t, &topics(&[("in", 5), ("table", 2)])).unwrap();
    assert_eq!(
        c.create
            .iter()
            .find(|t| t.name == "rep")
            .unwrap()
            .partitions,
        2
    );
    assert_eq!(c.subtopologies["1"].tasks, 2);
    assert_eq!(c.subtopologies["0"].tasks, 5);
}

/// Kafka names the missing internal topics in the order of a Java `HashMap` of them: six orders recorded from a real 4.4.0.
#[test]
fn missing_internal_topics_are_named_in_the_order_kafkas_hash_map_has_them() {
    use super::super::topology::java_hash_order;
    let recorded = [
        ("admin-94nc", true),
        ("missing-fw8p", false),
        ("restart-94nc", false),
        ("scale-94nc", false),
        ("wc-94nc", true),
        ("wc-kj4g", false),
    ];
    for (app, words_first) in recorded {
        let words = format!("java-streams-{app}-words-repartition");
        let counts = format!("java-streams-{app}-counts-changelog");
        let order = java_hash_order([counts.as_str(), words.as_str()]);
        let want = if words_first {
            [words.as_str(), counts.as_str()]
        } else {
            [counts.as_str(), words.as_str()]
        };
        assert_eq!(order, want, "{app}");
    }
    // Java's "a".hashCode() is 97 and "b" 98: buckets 1 and 2 of 16.
    assert_eq!(java_hash_order(["b", "a"]), ["a", "b"]);
}
