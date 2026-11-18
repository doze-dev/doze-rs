//! How many partitions each topic of a topology has or is to have: Kafka's `RepartitionTopics`,
//! `CopartitionedTopicsEnforcer` and `ChangelogTopics`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{Invalid, Topology, invalid, unexpected};
use crate::groups::consumer::Topics;

/// The partitions of a topic: what it has if it exists, else what has been decided for it.
pub(super) struct Counts<'a> {
    pub(super) topics: &'a Topics,
    pub(super) decided: HashMap<String, i32>,
}

impl Counts<'_> {
    pub(super) fn of(&self, name: &str) -> Option<i32> {
        self.topics
            .get(name)
            .map(|t| t.partitions)
            .or_else(|| self.decided.get(name).copied())
    }

    pub(super) fn must(&self, name: &str) -> Result<i32, Invalid> {
        self.of(name)
            .ok_or_else(|| unexpected(format!("Number of partitions is not set for topic: {name}")))
    }
}

/// Kafka's `RepartitionTopics.setup`: a fixed repartition topic has its own count; a sink not yet known takes the most
/// partitions of what its subtopology reads, round after round until every sink is known.
pub(super) fn repartition_counts(
    topology: &Topology,
    counts: &mut Counts<'_>,
) -> Result<(), Invalid> {
    for s in topology.subtopologies.values() {
        for r in s.repartition_sources.iter().filter(|r| r.partitions != 0) {
            counts.decided.insert(r.name.clone(), r.partitions);
        }
    }
    loop {
        let (mut needed, mut progress) = (false, false);
        for s in topology.subtopologies.values() {
            for sink in &s.repartition_sinks {
                if counts.decided.contains_key(sink) {
                    continue;
                }
                let from_repartition = s
                    .repartition_sources
                    .iter()
                    .filter_map(|r| counts.decided.get(&r.name).copied());
                let from_sources = s.sources.iter().filter_map(|t| counts.of(t));
                let most = from_repartition
                    .chain(from_sources)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .max();
                match most {
                    Some(n) => {
                        counts.decided.insert(sink.clone(), n);
                        progress = true;
                    }
                    None => needed = true,
                }
            }
        }
        if needed && !progress {
            return Err(invalid(
                "Failed to compute number of partitions for all repartition topics. There may be loops in the topology that cannot be resolved.",
            ));
        }
        if !needed {
            break;
        }
    }
    let unknown = topology
        .subtopologies
        .values()
        .flat_map(|s| &s.repartition_sources)
        .any(|r| !counts.decided.contains_key(&r.name));
    if unknown {
        return Err(invalid(
            "Failed to compute number of partitions for all repartition topics, because a repartition source topic is never used as a sink topic.",
        ));
    }
    Ok(())
}

/// Kafka's `CopartitionedTopicsEnforcer`, group by group. `Ok(Err(words))`: topics that are not partitioned alike.
pub(super) fn copartition(
    topology: &Topology,
    counts: &mut Counts<'_>,
) -> Result<Result<(), String>, Invalid> {
    let repartition = |fixed: bool| -> BTreeSet<&str> {
        topology
            .subtopologies
            .values()
            .flat_map(|s| &s.repartition_sources)
            .filter(|r| (r.partitions != 0) == fixed)
            .map(|r| r.name.as_str())
            .collect()
    };
    let (fixed, flexible) = (repartition(true), repartition(false));
    for s in topology.subtopologies.values() {
        for group in &s.copartition {
            let mut names = BTreeSet::new();
            for i in &group.sources {
                let t = usize::try_from(*i).ok().and_then(|i| s.sources.get(i));
                names.insert(
                    t.ok_or_else(|| unexpected("a copartition group names no source topic"))?
                        .as_str(),
                );
            }
            for i in &group.repartition_sources {
                let t = usize::try_from(*i)
                    .ok()
                    .and_then(|i| s.repartition_sources.get(i));
                names.insert(
                    t.ok_or_else(|| unexpected("a copartition group names no repartition topic"))?
                        .name
                        .as_str(),
                );
            }
            if let Err(words) = enforce(&names, &fixed, &flexible, counts)? {
                return Ok(Err(words));
            }
        }
    }
    Ok(Ok(()))
}

fn enforce(
    group: &BTreeSet<&str>,
    fixed: &BTreeSet<&str>,
    flexible: &BTreeSet<&str>,
    counts: &mut Counts<'_>,
) -> Result<Result<(), String>, Invalid> {
    let mut repartition = BTreeMap::new();
    let mut external = BTreeMap::new();
    for name in group {
        let n = counts.must(name)?;
        if fixed.contains(name) || flexible.contains(name) {
            repartition.insert(*name, n);
        } else {
            external.insert(*name, n);
        }
    }
    let differ = |m: &BTreeMap<&str, i32>| {
        let shown = m
            .iter()
            .map(|(t, n)| format!("{t}={n}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Following topics do not have the same number of partitions: [{{{shown}}}]")
    };
    let chosen = if external.is_empty() {
        if let Some(first) = fixed.first() {
            let first = counts.must(first)?;
            for t in fixed {
                if counts.must(t)? != first {
                    return Ok(Err(differ(&repartition)));
                }
            }
            first
        } else {
            let most = repartition.values().copied().max().unwrap_or(0);
            if most == 0 {
                let names = repartition.keys().copied().collect::<Vec<_>>().join(", ");
                return Err(invalid(format!(
                    "All topics in the copartition group had undefined partition number: [{names}]"
                )));
            }
            most
        }
    } else {
        let first = external.values().next().copied().unwrap_or(0);
        if external.values().any(|n| *n != first) {
            return Ok(Err(differ(&external)));
        }
        first
    };
    for (name, n) in repartition {
        counts.decided.insert(name.to_owned(), chosen);
        if fixed.contains(name) && n != chosen {
            return Ok(Err(format!(
                "Number of partitions [{n}] of repartition topic [{name}] doesn't match number of partitions [{chosen}] of the source topic."
            )));
        }
    }
    Ok(Ok(()))
}

/// Kafka's `ChangelogTopics.setup`: a changelog has as many partitions as the most its subtopology reads.
pub(super) fn changelog_counts(
    topology: &Topology,
    counts: &mut Counts<'_>,
) -> Result<(), Invalid> {
    for (id, s) in &topology.subtopologies {
        let mut most: Option<i32> = None;
        for t in s.inputs() {
            let n = counts.must(t)?;
            most = Some(most.map_or(n, |m| m.max(n)));
        }
        let most =
            most.ok_or_else(|| invalid(format!("No source topics found for subtopology {id}")))?;
        for c in &s.changelogs {
            counts.decided.insert(c.name.clone(), most);
        }
    }
    Ok(())
}
