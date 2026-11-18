//! A streams group's topology, as its first member sends it, and what it comes to against the topics there are (Kafka 4.4.0's
//! `InternalTopicManager.configureTopics`): how many tasks each subtopology has, how many partitions each internal topic
//! is to have, which of those are still to be made, and why the topology is not ready when it is not.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::groups::consumer::Topics;

mod counts;

use counts::{Counts, changelog_counts, copartition, repartition_counts};

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
    pub(crate) fn seen(&self, topics: &Topics) -> super::Seen {
        let mut out = BTreeMap::new();
        for s in self.subtopologies.values() {
            let names = s
                .inputs()
                .chain(s.changelogs.iter().map(|t| t.name.as_str()));
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

pub(super) fn invalid(words: impl Into<String>) -> Invalid {
    Invalid {
        words: words.into(),
        internal: false,
    }
}

pub(super) fn unexpected(words: impl Into<String>) -> Invalid {
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
    let (subtopologies, mut create) = subtopologies(topology, &counts)?;
    // Those that exist are not made; one that exists with other partitions makes the topology wrong.
    for s in topology.subtopologies.values() {
        let required = s
            .inputs()
            .chain(s.changelogs.iter().map(|t| t.name.as_str()));
        for name in required {
            let Some(t) = topics.get(name) else { continue };
            let Some(expected) = create.remove(name) else {
                continue;
            };
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

type Subtopologies = (BTreeMap<String, ConfiguredSub>, BTreeMap<String, Creatable>);

/// Each subtopology's tasks and internal topics, and every internal topic as it is to be made.
fn subtopologies(topology: &Topology, counts: &Counts<'_>) -> Result<Subtopologies, Invalid> {
    let mut subtopologies = BTreeMap::new();
    let mut create: BTreeMap<String, Creatable> = BTreeMap::new();
    for (id, s) in &topology.subtopologies {
        let tasks = s
            .inputs()
            .map(|t| counts.must(t))
            .try_fold(None, |most: Option<i32>, n| {
                n.map(|n| Some(most.map_or(n, |m| m.max(n))))
            })?;
        let tasks =
            tasks.ok_or_else(|| unexpected("Subtopology does not contain any source topics"))?;
        let mut internal = Vec::new();
        for info in s.repartition_sources.iter().chain(&s.changelogs) {
            let partitions = match info.partitions {
                0 => *counts.decided.get(&info.name).ok_or_else(|| {
                    unexpected(format!(
                        "Number of partitions must be set for topic {}",
                        info.name
                    ))
                })?,
                n => n,
            };
            internal.push((info.name.clone(), partitions));
            create.insert(
                info.name.clone(),
                Creatable {
                    name: info.name.clone(),
                    partitions,
                    replication: if info.replication == 0 {
                        -1
                    } else {
                        info.replication
                    },
                    configs: info.configs.clone(),
                },
            );
        }
        let stateful = !s.changelogs.is_empty();
        subtopologies.insert(
            id.clone(),
            ConfiguredSub {
                tasks,
                stateful,
                internal,
            },
        );
    }
    Ok((subtopologies, create))
}

fn not_ready(code: i8, detail: String) -> Configured {
    Configured {
        subtopologies: BTreeMap::new(),
        create: Vec::new(),
        not_ready: Some(NotReady { code, detail }),
    }
}
