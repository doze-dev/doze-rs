//! STR-10 to STR-17: the group epoch, the topology, the initial rebalance delay and the assignment interval.

use kafka_wire::ResponseError as E;

use super::super::status;
use super::super::topology::MISSING_INTERNAL_TOPICS;
use super::{World, beat, join, ready_topics, refused, topics, word_count};

/// What Kafka 4.4.0 answers a word count's first member (recorded from the oracle): epoch 1 with ASSIGNMENT_DELAYED and
/// MISSING_INTERNAL_TOPICS, empty task lists; epoch 1 again once the topics are made; epoch 3 with every task once the
/// delay is over.
#[test]
fn the_first_member_waits_out_the_initial_delay_at_epoch_one_and_is_given_its_tasks_at_three() {
    let mut w = World::new(topics(&[("in", 3)]));
    let a = w.send(&join("a"));
    assert_eq!(a.epoch, 1);
    let codes: Vec<i8> = a.status.iter().map(|s| s.0).collect();
    assert_eq!(codes, [status::ASSIGNMENT_DELAYED, MISSING_INTERNAL_TOPICS]);
    assert_eq!(a.status[0].1, "Assignment delayed due to the configured initial rebalance delay.");
    assert_eq!(a.tasks.as_ref().map(super::super::Tasks::is_empty), Some(true));
    assert_eq!(a.create.len(), 2);
    assert_eq!(a.endpoint_epoch, 0);
    assert_eq!(w.group.state(), "NotReady");
    // The handler makes the topics; the next heartbeat finds them.
    w.topics = ready_topics(3);
    w.advance(500);
    let a = w.send(&beat("a", 1, Some(w.held("a"))));
    assert_eq!((a.epoch, a.tasks.is_none()), (1, true));
    assert_eq!(a.status.len(), 1);
    assert_eq!(w.group.epoch, 3);
    assert_eq!(w.group.state(), "Assigning");
    w.advance(3_000);
    let a = w.send(&beat("a", 1, Some(w.held("a"))));
    assert_eq!(a.epoch, 3);
    assert_eq!(a.tasks.unwrap().active.len(), 6);
    assert!(a.status.is_empty());
    assert_eq!(w.group.state(), "Stable");
}

#[test]
fn without_an_initial_delay_targets_are_computed_at_once_and_then_no_oftener_than_the_interval() {
    let mut w = World::new(ready_topics(2));
    let mut hb = join("a");
    hb.timing.initial_delay_ms = 0;
    let a = w.send(&hb);
    assert_eq!(a.epoch, 2);
    assert_eq!(a.tasks.unwrap().active.len(), 4);
    let mut b = join("b");
    b.timing.initial_delay_ms = 0;
    w.advance(200);
    let b = w.send(&b);
    assert_eq!(b.status[0], (status::ASSIGNMENT_DELAYED, "Assignment delayed due to the configured assignment interval.".to_owned()));
    assert_eq!(w.group.state(), "Assigning");
    w.advance(1_000);
    let mut again = beat("b", b.epoch, Some(w.held("b")));
    again.timing.initial_delay_ms = 0;
    let b = w.send(&again);
    assert!(b.status.is_empty(), "{:?}", b.status);
    assert_eq!(w.group.assignment_epoch, w.group.epoch);
}

#[test]
fn a_different_topology_at_the_same_epoch_or_later_is_refused_and_an_older_one_is_told_it_is_stale() {
    let mut w = World::new(ready_topics(2));
    w.settle(&["a"]);
    let mut other = join("b");
    if let Some(t) = other.topology.as_mut() {
        t.subtopologies.remove("1");
    }
    assert_eq!(refused(&w.send(&other)), (E::InvalidRequest, "Topology updates are not supported yet."));
    // The group's topology at a later epoch: a member joining with the older one is let in, and told it is stale.
    let mut newer = word_count();
    newer.epoch = 5;
    w.group.topology = Some(newer);
    let b = w.send(&join("b"));
    assert!(b.error.is_none());
    assert_eq!(b.status[0], (status::STALE_TOPOLOGY, "The member's topology epoch 0 is behind the group's topology epoch 5.".to_owned()));
}

#[test]
fn changing_the_standby_replicas_moves_the_epoch_at_the_next_heartbeat() {
    let mut w = World::new(ready_topics(2));
    let last = w.settle(&["a"]);
    let before = w.group.epoch;
    let mut hb = beat("a", last["a"].epoch, Some(w.held("a")));
    let quiet = w.send(&hb);
    assert_eq!(w.group.epoch, before, "{quiet:?}");
    hb.timing.standbys = 1;
    w.send(&hb);
    assert_eq!(w.group.epoch, before + 1);
}

#[test]
fn a_topic_that_grows_moves_the_epoch_and_the_tasks_follow() {
    let mut w = World::new(ready_topics(2));
    let last = w.settle(&["a"]);
    let before = w.group.epoch;
    w.topics = topics(&[("in", 4), ("app-rep", 2), ("app-counts-changelog", 2)]);
    let a = w.send(&beat("a", last["a"].epoch, Some(w.held("a"))));
    assert_eq!(w.group.epoch, before + 1);
    // The repartition topic now has fewer partitions than the topology wants: not ready, nothing to run.
    assert_eq!(a.status.last().map(|s| s.0), Some(super::super::topology::INCORRECTLY_PARTITIONED_TOPICS));
    assert_eq!(w.group.state(), "NotReady");
}
