//! What the broker is told at start.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kafka_auth::Users;
pub use kafka_meta::TxnVersion;

/// The broker-wide values that a topic's own config overrides. Retention uses Kafka's way of saying "no limit":
/// -1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Defaults {
    /// Roll to a new segment at this size (`segment.bytes`).
    pub segment_bytes: i64,
    /// One index entry per this many bytes (`index.interval.bytes`).
    pub index_interval_bytes: i64,
    /// The largest record batch accepted (`max.message.bytes`).
    pub message_max_bytes: i64,
    /// Delete the oldest segments once a partition exceeds this (`retention.bytes`).
    pub retention_bytes: i64,
    /// Delete segments older than this (`retention.ms`).
    pub retention_ms: i64,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            segment_bytes: 128 << 20,
            index_interval_bytes: 4 << 10,
            message_max_bytes: 1_048_588,
            retention_bytes: -1,
            retention_ms: -1,
        }
    }
}

/// The broker's configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where everything is kept: the metadata database and a directory per partition.
    pub data_dir: PathBuf,
    /// This broker's id, the only one in the cluster.
    pub node_id: i32,
    /// The cluster id reported in Metadata.
    pub cluster_id: String,
    /// Whether a Metadata request may create the topics it asks for.
    pub auto_create_topics: bool,
    /// Partitions of an auto-created topic, and of a CreateTopics that asks for the default.
    pub default_partitions: i32,
    /// The most partitions one topic may have: each is a directory and, in use, open files.
    pub max_partitions: i32,
    /// How many partitions stay open at once (each holds files open): past it the least recently used are closed,
    /// and open again when asked for. Zero keeps every partition open once opened.
    pub max_open_partitions: usize,
    /// A partition nothing has asked for in this long, and that nothing holds, is closed (flushed, its files let go) and
    /// opens again when it is next asked for: an idle broker holds no partition's files. Five minutes; zero keeps an opened
    /// partition open until `max_open_partitions` closes it.
    pub partition_idle_close: Duration,
    /// How often every topic's retention is applied to its partitions (`log.retention.check.interval.ms`).
    pub log_retention_check_every: Duration,
    /// How often compacted topics are looked at to be cleaned (`log.cleaner.backoff.ms`).
    pub log_cleaner_every: Duration,
    /// Whether compacted topics are cleaned at all (`log.cleaner.enable`).
    pub log_cleaner_enable: bool,
    /// The most bytes of keys one round of cleaning holds (`log.cleaner.dedupe.buffer.size`, which is 128 MiB in Kafka).
    pub cleaner_map_bytes: usize,
    /// How many files the open partitions may hold open between them (a partition holds two for each segment it has):
    /// past it the least recently used are closed, as past `max_open_partitions`. Zero: no bound of its own.
    pub max_open_descriptors: usize,
    /// Broker-wide values that topics override.
    pub defaults: Defaults,
    /// Appends that did not ask to be durable (`acks=1`) are flushed this long after the first of them. Zero
    /// flushes each group at once.
    pub fsync_interval: Duration,
    /// How long the first rebalance of an empty consumer group waits for more members to arrive, so that members
    /// starting together share a generation. Zero: not at all.
    pub group_initial_rebalance_delay: Duration,
    /// How long committed offsets are kept after their group has gone, or after they were committed if it never had
    /// members. Zero: for ever.
    pub offsets_retention: Duration,
    /// How long a transactional id with no open transaction is kept after its last change (`transactional.id.expiration.ms`).
    pub transactional_id_expiration: Duration,
    /// How often the offsets are looked at; `None`: a hundredth of the retention, between 50 ms and ten minutes.
    pub offsets_check_every: Option<Duration>,
    /// How long a member of a consumer group of the new protocol may be silent before it is removed.
    pub consumer_session_timeout: Duration,
    /// The least time between two computations of a consumer group's target assignment (Kafka's
    /// `group.consumer.assignment.interval.ms`).
    pub consumer_assignment_interval: Duration,
    /// What a resource that no ACL covers allows (`allow.everyone.if.no.acl.found`): true, so that a broker with no
    /// ACLs refuses nothing. False is Kafka's own default.
    pub acl_default_allow: bool,
    /// Principals no ACL refuses anything (`super.users`), each as `Type:name`.
    pub super_users: BTreeSet<String>,
    /// The users that can log in. The broker loads the credentials stored over the API into it when it opens and keeps
    /// it up to date; the server's authenticator shares it, so a change applies to the next login.
    pub users: Arc<Users>,
    /// Whether the broker serves client telemetry (KIP-714: GetTelemetrySubscriptions and PushTelemetry) and says so in
    /// ApiVersions. On by default, as the recordings have it; off, a client that sends either is disconnected as for any
    /// API the broker does not serve.
    pub client_telemetry: bool,
    /// The `transaction.version` the broker finalizes (`Profile::transaction_version`): how it ends a transaction on its
    /// own, when the transaction outlives its timeout or a new owner takes its id. At KIP-890's second part (Kafka 4) the
    /// epoch is raised as EndTxn v5 raises it, and the epoch before is kept, so the producer that timed out may take the
    /// id back by saying it (KIP-360); at the classic protocol (Kafka 1 to 3) it is not.
    pub transaction_version: TxnVersion,
    /// The share groups' assignment interval when a group sets no `share.assignment.interval.ms`; `None`: Kafka's 1 s. A
    /// knob for tests, which would otherwise wait out the interval.
    pub share_assignment_interval: Option<Duration>,
    /// Whether the versions Kafka marks unstable are offered (Kafka's internal `unstable.api.versions.enable`, false):
    /// in Kafka 4.4.0 that is InitProducerId v6 (KIP-939, two-phase commit) alone.
    pub unstable_api_versions: bool,
    /// Whether a producer may take part in a two-phase commit (KIP-939: InitProducerId v6 with `Enable2Pc`), whose
    /// transactions never time out (`transaction.two.phase.commit.enable`, false as in Kafka 4.4.0). Off, such a request
    /// is TRANSACTIONAL_ID_AUTHORIZATION_FAILED, as Kafka answers it.
    pub transaction_two_phase_commit: bool,
}

impl Config {
    /// The defaults, keeping data under `data_dir`.
    #[must_use]
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            node_id: 1,
            cluster_id: "doze-kafka".to_owned(),
            auto_create_topics: true,
            default_partitions: 1,
            max_partitions: 1000,
            max_open_partitions: 512,
            partition_idle_close: Duration::from_secs(300),
            log_retention_check_every: Duration::from_secs(300),
            log_cleaner_every: Duration::from_secs(15),
            log_cleaner_enable: true,
            cleaner_map_bytes: 16 << 20,
            max_open_descriptors: 0,
            defaults: Defaults::default(),
            fsync_interval: Duration::from_millis(500),
            group_initial_rebalance_delay: Duration::from_millis(200),
            offsets_retention: Duration::from_secs(7 * 24 * 3600),
            transactional_id_expiration: Duration::from_secs(7 * 24 * 3600),
            offsets_check_every: None,
            consumer_session_timeout: Duration::from_secs(45),
            consumer_assignment_interval: Duration::from_secs(1),
            acl_default_allow: true,
            super_users: BTreeSet::new(),
            users: Arc::new(Users::none()),
            client_telemetry: true,
            transaction_version: TxnVersion::EpochBump,
            share_assignment_interval: None,
            unstable_api_versions: false,
            transaction_two_phase_commit: false,
        }
    }
}
