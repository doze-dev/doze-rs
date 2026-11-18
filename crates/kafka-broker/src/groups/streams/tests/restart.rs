//! STR-80 to STR-82: what a streams group keeps across a restart: its epochs and its topology, written as it seats a member
//! and as it empties, and read back into an empty group the members join again.

use super::super::stored::{kept_of, life_of};
use super::super::{StreamsGroup, Tasks};
use super::{World, beat, join, ready_topics, word_count};

#[test]
fn the_record_keeps_the_epochs_and_the_topology_whole() {
    let mut w = World::new(ready_topics(2));
    w.settle(&["a"]);
    w.send(&beat("a", -1, None));
    let kept = w.group.kept(None);
    let life = life_of(&kept);
    assert_eq!(kept_of(&life), kept);
    assert_eq!(kept.topology, Some(word_count()));
    assert_eq!(
        (kept.epoch, kept.assignment_epoch),
        (w.group.epoch, w.group.assignment_epoch)
    );
}

#[test]
fn a_group_seating_its_first_member_keeps_the_topology_the_join_brings() {
    let g = StreamsGroup::new();
    let kept = g.kept(join("a").topology.as_ref());
    assert_eq!(kept.topology, Some(word_count()));
    assert_eq!((kept.epoch, kept.assignment_epoch), (1, 1));
}

#[test]
fn a_restored_group_is_empty_with_its_topology_and_its_members_join_again_at_a_later_epoch() {
    let mut w = World::new(ready_topics(2));
    w.settle(&["a"]);
    w.send(&beat("a", -1, None));
    let before = w.group.epoch;
    let restored = StreamsGroup::restored(kept_of(&life_of(&w.group.kept(None))));
    let view = restored.view();
    assert_eq!((view.state, view.epoch), ("Empty", before));
    assert_eq!(view.topology, Some(word_count()));
    // A member of the old run is unknown, and joins again.
    let mut w2 = World::new(ready_topics(2));
    w2.group = restored;
    let stale = w2.send(&beat("a", 3, Some(Tasks::default())));
    assert_eq!(
        stale.error.as_ref().map(|e| e.0),
        Some(kafka_wire::ResponseError::UnknownMemberId)
    );
    let last = w2.settle(&["a"]);
    assert!(last["a"].epoch > before);
    assert_eq!(w2.held("a").active.len(), 4);
}
