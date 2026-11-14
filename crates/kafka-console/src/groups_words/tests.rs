//! The sentences of the Groups pages against the larder: what they say, and what they refuse to say.

use super::*;
use crate::fixtures_groups as world;
use crate::model::GroupSpan;

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
