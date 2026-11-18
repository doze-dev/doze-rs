//! A streams group's record in the metadata database, and back.

use kafka_meta::{StoredSubtopology, StoredTopic, StoredTopology, StreamsLife};

use super::Kept;
use super::topology::{Copartition, Subtopology, TopicInfo, Topology};

fn topic_of(t: &StoredTopic) -> TopicInfo {
    TopicInfo {
        name: t.name.clone(),
        partitions: t.partitions,
        replication: t.replication,
        configs: t.configs.clone(),
    }
}

fn stored_topic(t: &TopicInfo) -> StoredTopic {
    StoredTopic {
        name: t.name.clone(),
        partitions: t.partitions,
        replication: t.replication,
        configs: t.configs.clone(),
    }
}

/// The topology a record keeps.
#[must_use]
pub(crate) fn topology_of(t: &StoredTopology) -> Topology {
    Topology {
        epoch: t.epoch,
        subtopologies: t
            .subtopologies
            .iter()
            .map(|s| {
                let sub = Subtopology {
                    sources: s.sources.clone(),
                    repartition_sinks: s.repartition_sinks.clone(),
                    repartition_sources: s.repartition_sources.iter().map(topic_of).collect(),
                    changelogs: s.changelogs.iter().map(topic_of).collect(),
                    copartition: s
                        .copartition
                        .iter()
                        .map(|(sources, repartition_sources)| Copartition {
                            sources: sources.clone(),
                            repartition_sources: repartition_sources.clone(),
                        })
                        .collect(),
                };
                (s.id.clone(), sub)
            })
            .collect(),
    }
}

fn stored(t: &Topology) -> StoredTopology {
    StoredTopology {
        epoch: t.epoch,
        subtopologies: t
            .subtopologies
            .iter()
            .map(|(id, s)| StoredSubtopology {
                id: id.clone(),
                sources: s.sources.clone(),
                repartition_sinks: s.repartition_sinks.clone(),
                repartition_sources: s.repartition_sources.iter().map(stored_topic).collect(),
                changelogs: s.changelogs.iter().map(stored_topic).collect(),
                copartition: s
                    .copartition
                    .iter()
                    .map(|c| (c.sources.clone(), c.repartition_sources.clone()))
                    .collect(),
            })
            .collect(),
    }
}

/// What a group keeps, from its record.
#[must_use]
pub(crate) fn kept_of(life: &StreamsLife) -> Kept {
    Kept {
        epoch: life.epoch,
        assignment_epoch: life.assignment_epoch,
        topology: life.topology.as_ref().map(topology_of),
    }
}

/// The record of what a group keeps.
#[must_use]
pub(crate) fn life_of(kept: &Kept) -> StreamsLife {
    StreamsLife {
        epoch: kept.epoch,
        assignment_epoch: kept.assignment_epoch,
        topology: kept.topology.as_ref().map(stored),
    }
}
