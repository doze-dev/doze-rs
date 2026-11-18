//! The view-models of a streams group (KIP-1071): its topology (subtopologies and the internal topics they make), why it is
//! not ready when it is not, and each member's tasks by role, held and targeted.

/// Why a streams group's topology is not ready: the status its members are told.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StreamsNotReady {
    pub code: i8,
    /// Kafka's name of the status (`MISSING_SOURCE_TOPICS`, `MISSING_INTERNAL_TOPICS`, `INCORRECTLY_PARTITIONED_TOPICS`).
    pub name: String,
    /// Kafka's words (`Source topics words are missing.`).
    pub detail: String,
}

/// One subtopology: what it reads, what it writes for another to read, its state stores' changelogs and its tasks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StreamsSubtopologyRow {
    pub id: String,
    pub sources: Vec<String>,
    pub repartition_sinks: Vec<String>,
    pub repartition_sources: Vec<String>,
    pub changelogs: Vec<String>,
    /// How many tasks it has (the most partitions of a topic it reads); `None` while the topology is not ready.
    pub tasks: Option<i32>,
}

/// Which kind of internal topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum StreamsTopicKind {
    /// Written by one subtopology and read by another (records re-keyed).
    Repartition,
    /// A state store's copy, compacted, read back when a task moves.
    Changelog,
}

/// An internal topic a topology makes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StreamsInternalTopic {
    pub name: String,
    pub kind: StreamsTopicKind,
    /// The subtopology that reads it (repartition) or keeps it (changelog).
    pub subtopology: String,
    /// The partitions the topology works out for it; `None` while it is not ready.
    pub partitions: Option<i32>,
    /// Its partitions as Metadata says; `None` when it does not exist (yet).
    pub exists: Option<usize>,
    /// The configs the application asked it to have.
    pub configs: Vec<(String, String)>,
}

/// Tasks by role, each as Kafka writes a task id (`<subtopology>_<partition>`, `1_0`).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StreamsRoles {
    pub active: Vec<String>,
    pub standby: Vec<String>,
    pub warmup: Vec<String>,
}

/// One member: a stream thread of a Kafka Streams process.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StreamsMemberRow {
    pub id: String,
    pub process: String,
    pub client_id: String,
    pub host: String,
    /// `host:port` of its interactive queries, if it said one.
    pub endpoint: Option<String>,
    pub instance: Option<String>,
    pub rack: Option<String>,
    pub epoch: i32,
    pub topology_epoch: i32,
    /// `STABLE`, `UNREVOKED_TASKS` (told to give up tasks, not yet done) or `UNRELEASED_TASKS` (waiting for tasks others hold).
    pub stage: String,
    /// What it runs now.
    pub holds: StreamsRoles,
    /// What it is to run.
    pub target: StreamsRoles,
    /// What it was told to give up and has not said it has.
    pub revoking: StreamsRoles,
}

/// A streams group's page: the topology, the internal topics and the members' tasks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GroupStreamsView {
    /// Kafka's state word: `Empty`, `NotReady`, `Assigning`, `Reconciling`, `Stable`.
    pub state: String,
    pub epoch: i32,
    pub assignment_epoch: i32,
    /// `None` before a member sent a topology.
    pub topology_epoch: Option<i32>,
    pub not_ready: Option<StreamsNotReady>,
    pub subtopologies: Vec<StreamsSubtopologyRow>,
    pub internal: Vec<StreamsInternalTopic>,
    pub members: Vec<StreamsMemberRow>,
    /// The broker has the group in memory (else it was described from its kept record: no member, as after a restart).
    pub live: bool,
}
