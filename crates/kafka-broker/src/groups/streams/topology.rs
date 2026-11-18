//! A streams group's topology, as its first member sends it, and what it comes to against the topics there are (Kafka 4.4.0's
//! `InternalTopicManager.configureTopics`): how many tasks each subtopology has, how many partitions each internal topic
//! is to have, which of those are still to be made, and why the topology is not ready when it is not.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::groups::consumer::Topics;

/// An internal topic as the topology describes it (Kafka's `TopicInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopicInfo {
    pub(crate) name: String,
    /// 0: whatever the topology works out.
    pub(crate) partitions: i32,
    /// 0: the broker's default.
    pub(crate) replication: i16,
    /// Topic configs, as the client sent them.
    pub(crate) configs: Vec<(String, String)>,
}

/// Source topics that must be partitioned alike, by their index in the subtopology's lists.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Copartition {
    pub(crate) sources: Vec<i16>,
    pub(crate) repartition_sources: Vec<i16>,
}

/// One subtopology (Kafka's `StreamsGroupTopologyValue.Subtopology`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Subtopology {
    pub(crate) sources: Vec<String>,
    pub(crate) repartition_sinks: Vec<String>,
    pub(crate) repartition_sources: Vec<TopicInfo>,
    pub(crate) changelogs: Vec<TopicInfo>,
    pub(crate) copartition: Vec<Copartition>,
}

impl Subtopology {
    /// The topics its tasks read: its sources and its repartition sources.
    pub(crate) fn inputs(&self) -> impl Iterator<Item = &str> {
        self.sources
            .iter()
            .map(String::as_str)
            .chain(self.repartition_sources.iter().map(|t| t.name.as_str()))
    }
}

/// The topology of a streams application: its epoch and its subtopologies by id. Two are the same only if every list is
/// the same in the same order (Kafka compares the records).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Topology {
    pub(crate) epoch: i32,
    pub(crate) subtopologies: BTreeMap<String, Subtopology>,
}

impl Topology {
    /// The topics whose partitions the configuration depends on, with their id and count now (Kafka's metadata hash):
    /// the sources, repartition sources and changelogs that exist.
    pub(crate) fn seen(&self, topics: &Topics) -> BTreeMap<String, (kafka_wire::Uuid, i32)> {
        let mut out = BTreeMap::new();
        for s in self.subtopologies.values() {
            let names = s.inputs().chain(s.changelogs.iter().map(|t| t.name.as_str()));
            for name in names {
                if let Some(t) = topics.get(name) {
                    out.insert(name.to_owned(), (t.id, t.partitions));
                }
            }
        }
        out
    }
}

/// Why a topology is not ready: a status code of the heartbeat answer (KIP-1071) and its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotReady {
    pub(crate) code: i8,
    pub(crate) detail: String,
}

pub(crate) const MISSING_SOURCE_TOPICS: i8 = 1;
pub(crate) const INCORRECTLY_PARTITIONED_TOPICS: i8 = 2;
pub(crate) const MISSING_INTERNAL_TOPICS: i8 = 3;

/// A topology Kafka refuses outright: STREAMS_INVALID_TOPOLOGY with these words, or (`internal`) a state Kafka's
/// coordinator does not expect, which it answers as UNKNOWN_SERVER_ERROR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invalid {
    pub(crate) words: String,
    pub(crate) internal: bool,
}

fn invalid(words: impl Into<String>) -> Invalid {
    Invalid {
        words: words.into(),
        internal: false,
    }
}

fn unexpected(words: impl Into<String>) -> Invalid {
    Invalid {
        words: words.into(),
        internal: true,
    }
}

/// An internal topic to be made: its name, partitions, replication (-1: the broker's default) and configs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Creatable {
    pub(crate) name: String,
    pub(crate) partitions: i32,
    pub(crate) replication: i16,
    pub(crate) configs: Vec<(String, String)>,
}

/// A subtopology against the topics there are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfiguredSub {
    /// How many tasks it has: the most partitions of a topic it reads.
    pub(crate) tasks: i32,
    /// It keeps state in a changelog, so its tasks have standbys.
    pub(crate) stateful: bool,
    /// Each internal topic of it (repartition sources, then changelogs) with the partitions it is to have.
    pub(crate) internal: Vec<(String, i32)>,
}

/// What a topology comes to (Kafka's `ConfiguredTopology`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Configured {
    /// Empty when source topics are missing or a topic is wrongly partitioned.
    pub(crate) subtopologies: BTreeMap<String, ConfiguredSub>,
    /// The internal topics to make, by name.
    pub(crate) create: Vec<Creatable>,
    /// Why it is not ready, if it is not.
    pub(crate) not_ready: Option<NotReady>,
}

impl Configured {
    pub(crate) fn ready(&self) -> bool {
        self.not_ready.is_none()
    }
}

/// Kafka's `summarizeTopics`: the first three, then how many more.
pub(crate) fn summarize<'a>(topics: impl IntoIterator<Item = &'a str>) -> String {
    let all: Vec<&str> = topics.into_iter().collect();
    if all.is_empty() {
        return "<none>".to_owned();
    }
    let shown = all.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    match all.len() {
        n if n > 3 => format!("{shown} and {} additional topics", n - 3),
        _ => shown,
    }
}

/// The order a `java.util.HashMap` of these names iterates in (Kafka lists the missing internal topics from one): by
/// bucket, the bucket being the spread `String.hashCode` masked by the table's size (16, doubled past three quarters
/// full), and in the order given within a bucket.
pub(crate) fn java_hash_order<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut all: Vec<&str> = names.into_iter().collect();
    let mut capacity: usize = 16;
    while all.len() * 4 > capacity * 3 {
        capacity *= 2;
    }
    let bucket = |name: &str| {
        let h = name
            .encode_utf16()
            .fold(0_i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)));
        // Java's `h ^ (h >>> 16)`: the high bits folded into the low, as unsigned.
        let spread = h.cast_unsigned() ^ (h.cast_unsigned() >> 16);
        usize::try_from(spread).unwrap_or(0) & (capacity - 1)
    };
    all.sort_by_key(|n| bucket(n));
    all
}

/// The partitions of a topic: what it has if it exists, else what has been decided for it.
struct Counts<'a> {
    topics: &'a Topics,
    decided: HashMap<String, i32>,
}

impl Counts<'_> {
    fn of(&self, name: &str) -> Option<i32> {
        self.topics
            .get(name)
            .map(|t| t.partitions)
            .or_else(|| self.decided.get(name).copied())
    }

    fn must(&self, name: &str) -> Result<i32, Invalid> {
        self.of(name)
            .ok_or_else(|| unexpected(format!("Number of partitions is not set for topic: {name}")))
    }
}

/// Configures `topology` against `topics`. `Err`: a topology Kafka refuses outright.
pub(crate) fn configure(topology: &Topology, topics: &Topics) -> Result<Configured, Invalid> {
    let missing: BTreeSet<&str> = topology
        .subtopologies
        .values()
        .flat_map(|s| s.sources.iter().map(String::as_str))
        .filter(|t| topics.get(t).is_none())
        .collect();
    if !missing.is_empty() {
        return Ok(not_ready(
            MISSING_SOURCE_TOPICS,
            format!("Source topics {} are missing.", summarize(missing)),
        ));
    }
    let mut counts = Counts {
        topics,
        decided: HashMap::new(),
    };
    repartition_counts(topology, &mut counts)?;
    if let Err(wrong) = copartition(topology, &mut counts)? {
        return Ok(not_ready(INCORRECTLY_PARTITIONED_TOPICS, wrong));
    }
    changelog_counts(topology, &mut counts)?;
    let mut subtopologies = BTreeMap::new();
    let mut create: BTreeMap<String, Creatable> = BTreeMap::new();
    for (id, s) in &topology.subtopologies {
        let tasks = s.inputs().map(|t| counts.must(t)).try_fold(None, |most: Option<i32>, n| {
            n.map(|n| Some(most.map_or(n, |m| m.max(n))))
        })?;
        let tasks = tasks.ok_or_else(|| unexpected("Subtopology does not contain any source topics"))?;
        let mut internal = Vec::new();
        for info in s.repartition_sources.iter().chain(&s.changelogs) {
            let partitions = match info.partitions {
                0 => *counts.decided.get(&info.name).ok_or_else(|| {
                    unexpected(format!("Number of partitions must be set for topic {}", info.name))
                })?,
                n => n,
            };
            internal.push((info.name.clone(), partitions));
            create.insert(
                info.name.clone(),
                Creatable {
                    name: info.name.clone(),
                    partitions,
                    replication: if info.replication == 0 { -1 } else { info.replication },
                    configs: info.configs.clone(),
                },
            );
        }
        let stateful = !s.changelogs.is_empty();
        subtopologies.insert(id.clone(), ConfiguredSub { tasks, stateful, internal });
    }
    // Those that exist are not made; one that exists with other partitions makes the topology wrong.
    for s in topology.subtopologies.values() {
        let required = s.inputs().chain(s.changelogs.iter().map(|t| t.name.as_str()));
        for name in required {
            let Some(t) = topics.get(name) else { continue };
            let Some(expected) = create.remove(name) else { continue };
            if t.partitions != expected.partitions {
                return Ok(not_ready(
                    INCORRECTLY_PARTITIONED_TOPICS,
                    format!(
                        "Existing topic {name} has different number of partitions: expected {}, found {}",
                        expected.partitions, t.partitions
                    ),
                ));
            }
        }
    }
    let not_ready = (!create.is_empty()).then(|| NotReady {
        code: MISSING_INTERNAL_TOPICS,
        detail: format!(
            "Internal topics are missing: {}",
            summarize(java_hash_order(create.keys().map(String::as_str)))
        ),
    });
    Ok(Configured {
        subtopologies,
        create: create.into_values().collect(),
        not_ready,
    })
}

fn not_ready(code: i8, detail: String) -> Configured {
    Configured {
        subtopologies: BTreeMap::new(),
        create: Vec::new(),
        not_ready: Some(NotReady { code, detail }),
    }
}

/// Kafka's `RepartitionTopics.setup`: a fixed repartition topic has its own count; a sink not yet known takes the most
/// partitions of what its subtopology reads, round after round until every sink is known.
fn repartition_counts(topology: &Topology, counts: &mut Counts<'_>) -> Result<(), Invalid> {
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
                let most = from_repartition.chain(from_sources).collect::<Vec<_>>().into_iter().max();
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
fn copartition(topology: &Topology, counts: &mut Counts<'_>) -> Result<Result<(), String>, Invalid> {
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
                names.insert(t.ok_or_else(|| unexpected("a copartition group names no source topic"))?.as_str());
            }
            for i in &group.repartition_sources {
                let t = usize::try_from(*i).ok().and_then(|i| s.repartition_sources.get(i));
                names.insert(t.ok_or_else(|| unexpected("a copartition group names no repartition topic"))?.name.as_str());
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
        let shown = m.iter().map(|(t, n)| format!("{t}={n}")).collect::<Vec<_>>().join(", ");
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
fn changelog_counts(topology: &Topology, counts: &mut Counts<'_>) -> Result<(), Invalid> {
    for (id, s) in &topology.subtopologies {
        let mut most: Option<i32> = None;
        for t in s.inputs() {
            let n = counts.must(t)?;
            most = Some(most.map_or(n, |m| m.max(n)));
        }
        let most = most.ok_or_else(|| invalid(format!("No source topics found for subtopology {id}")))?;
        for c in &s.changelogs {
            counts.decided.insert(c.name.clone(), most);
        }
    }
    Ok(())
}
