//! The command line.
//!
//! The flags that matter are here. The ones of parts that come in later phases are
//! accepted and ignored, hidden from the help, so that the conformance harness, which passes them all, can run the
//! scenarios of the parts that exist. A flag that would make the broker safer or different in a way a user would
//! rely on is refused instead when it cannot be honoured: a broker that ignored `--tls-cert` would send in the
//! clear (so they need the `tls` feature, and are refused without it). SASL (PLAIN, SCRAM-SHA-256, SCRAM-SHA-512) is real (`--sasl-mechanisms`, `--sasl-user`); the passwords of `--sasl-user` are kept
//! out of every `Debug` output and of every message.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, ValueEnum};

use crate::duration;

/// How log lines are written.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum LogFormat {
    Text,
    Json,
}

/// A Kafka broker for local development: one node, one binary, data on disk.
#[derive(Debug, Parser)]
#[command(name = "doze-kafka", version, about)]
pub(crate) struct Args {
    /// Serve on this host:port.
    #[arg(long, default_value = "127.0.0.1:9092")]
    pub(crate) listen: SocketAddr,
    /// Host advertised to clients in Metadata (default: the address itself, or localhost for a wildcard).
    #[arg(long)]
    advertised_host: Option<String>,
    /// Port advertised to clients (default: the port actually bound).
    #[arg(long)]
    advertised_port: Option<u16>,
    /// Directory for the metadata database and the partition logs.
    #[arg(long, default_value = "./data")]
    pub(crate) data_dir: PathBuf,
    /// Roll a segment once it exceeds this size.
    #[arg(long, default_value_t = 128 << 20)]
    segment_bytes: i64,
    /// Add one sparse index entry per this many bytes of log.
    #[arg(long, default_value_t = 4 << 10)]
    index_interval_bytes: i64,
    /// Delete old segments once a partition exceeds this size (0 = unbounded).
    #[arg(long, default_value_t = 0)]
    retention_bytes: i64,
    /// Delete segments older than this many milliseconds (0 = unbounded).
    #[arg(long, default_value_t = 0)]
    retention_ms: i64,
    /// How soon after a write that did not ask to be durable (acks=1) it is flushed (0 = at once).
    #[arg(long, default_value = "500ms", value_parser = duration::parse)]
    fsync_interval: Duration,
    /// How long committed offsets are kept once their group has no members (0 = for ever).
    #[arg(long, default_value = "168h", value_parser = duration::parse)]
    offsets_retention: Duration,
    /// How often the retention of every topic is applied to its partitions (must be above 0).
    #[arg(long, default_value = "5m", value_parser = duration::parse)]
    log_retention_check_interval: Duration,
    /// How often compacted topics are looked at to be cleaned (must be above 0).
    #[arg(long, default_value = "15s", value_parser = duration::parse)]
    log_cleaner_interval: Duration,
    /// Clean topics whose cleanup.policy says compact.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
    log_cleaner_enable: bool,
    /// How long the first rebalance of an empty consumer group waits for more members to join (0 = not at all).
    #[arg(long, default_value = "200ms", value_parser = duration::parse)]
    group_initial_rebalance_delay: Duration,
    /// How long a member of a consumer group of the new protocol (KIP-848) may be silent before it is removed.
    #[arg(long, default_value = "45s", value_parser = duration::parse)]
    consumer_session_timeout: Duration,
    /// Accept the metrics clients push about themselves (KIP-714: GetTelemetrySubscriptions and PushTelemetry) and say so in
    /// ApiVersions. Clients that support it then report every five seconds; the last report of each is kept in memory,
    /// bounded (128 clients, 4 MiB), and is never read, shown or logged. `--client-telemetry=false` stops advertising the
    /// two APIs (Kafka 4 only: older versions have none).
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
    client_telemetry: bool,
    /// Also offer the protocol versions Kafka marks unstable, as Kafka's internal `unstable.api.versions.enable` does (off,
    /// as in Kafka). In Kafka 4.4.0 that is InitProducerId v6 alone: the two-phase commit of KIP-939, which no released client
    /// sends yet.
    #[arg(long, default_value_t = false, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
    unstable_api_versions: bool,
    /// Let producers take part in a two-phase commit (KIP-939, Kafka's `transaction.two.phase.commit.enable`; off, as in
    /// Kafka): their transactions never time out. It needs `--unstable-api-versions` for a client to ask for one.
    #[arg(long, default_value_t = false, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
    transaction_two_phase_commit: bool,
    /// Kafka major version to stand for (1-4).
    #[arg(long, default_value_t = 4)]
    kafka_version: u8,
    /// Create unknown topics when a client asks about them.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
    auto_create_topics: bool,
    /// Partition count for auto-created topics.
    #[arg(long, default_value_t = 1)]
    default_partitions: i32,
    /// Most partitions one topic may have (each is a directory and, in use, open files).
    #[arg(long, default_value_t = 1000)]
    max_partitions: i32,
    /// How many partitions stay open at once (each holds files open); past it the least recently used are closed
    /// and open again when asked for (0 = never closed).
    #[arg(long, default_value_t = 512)]
    max_open_partitions: usize,
    /// Largest record batch a topic accepts unless it sets max.message.bytes.
    #[arg(long, default_value_t = 1_048_588)]
    message_max_bytes: i64,
    /// Threads running the broker's work (each idle one is memory).
    #[arg(long, default_value_t = 2)]
    pub(crate) worker_threads: usize,
    /// Log verbosity: error, warn, info, debug or trace (or levels per target, as in warn,kafka_broker=debug; RUST_LOG, if
    /// set, wins, in the same form).
    #[arg(long, default_value = "info")]
    pub(crate) log_level: String,
    /// Log format.
    #[arg(long, value_enum, default_value = "text")]
    pub(crate) log_format: LogFormat,
    /// Who may log in and what they may do (`cli/login_flags.rs`).
    #[command(flatten)]
    pub(crate) login: login_flags::LoginFlags,

    /// The key delegation tokens are signed with (Kafka's delegation.token.secret.key); tokens are off without it. It shows in
    /// `ps`: DOZE_KAFKA_DELEGATION_TOKEN_SECRET_KEY is read too. Tokens are used over SASL only; a new key voids them all.
    #[arg(long, value_name = "SECRET")]
    delegation_token_secret_key: Option<String>,
    /// The longest a delegation token can live, renewals and all (Kafka's delegation.token.max.lifetime.ms, 7 days).
    #[arg(long, default_value = "168h", value_name = "DURATION", value_parser = duration::parse)]
    delegation_token_max_lifetime: Duration,
    /// How long a delegation token lives from its making or its last renewal, unless less is asked for (Kafka's
    /// delegation.token.expiry.time.ms, 1 day).
    #[arg(long, default_value = "24h", value_name = "DURATION", value_parser = duration::parse)]
    delegation_token_expiry_time: Duration,
    /// How often expired delegation tokens are removed (Kafka's delegation.token.expiry.check.interval.ms, 1 hour); until then
    /// an expired token is still described and can still log in, as in Kafka.
    #[arg(long, default_value = "1h", value_name = "DURATION", value_parser = duration::parse)]
    delegation_token_expiry_check_interval: Duration,

    // Accepted and ignored until the phase that has it.
    #[arg(long, hide = true)]
    meta_db: Option<String>,
    /// The console flags (`cli/console_flags.rs`).
    #[command(flatten)]
    pub(crate) console: console_flags::ConsoleFlags,

    /// PEM certificate chain (leaf first) to serve TLS with. Needs --tls-key.
    #[cfg(feature = "tls")]
    #[arg(long, value_name = "FILE")]
    tls_cert: Option<PathBuf>,
    /// PEM private key of the certificate (PKCS#8, SEC1 or RSA, not encrypted). Needs --tls-cert.
    #[cfg(feature = "tls")]
    #[arg(long, value_name = "FILE")]
    tls_key: Option<PathBuf>,
    // Without the feature the two flags are accepted, hidden, to be refused with a message that names the feature.
    /// PEM certificate chain (leaf first) to serve TLS with. This build has no TLS and refuses the flag.
    #[cfg(not(feature = "tls"))]
    #[arg(long, hide = true)]
    tls_cert: Option<String>,
    /// PEM private key of the certificate. This build has no TLS and refuses the flag.
    #[cfg(not(feature = "tls"))]
    #[arg(long, hide = true)]
    tls_key: Option<String>,

    /// The schema registry's flags (`cli/registry.rs`).
    #[command(flatten)]
    pub(crate) registry: registry::RegistryArgs,
}

mod acl;
mod console;
mod console_flags;
mod derived;
mod login_flags;
mod major;
mod settings;
pub(crate) use console::Console;
#[cfg(feature = "console")]
pub(crate) use console::DEFAULT_ADDR as DEFAULT_CONSOLE_ADDR;
#[cfg(feature = "console")]
pub(crate) use console::stored_acls;
pub(crate) use settings::Settings;
mod sasl;
pub(crate) use sasl::NO_USERS;
mod registry;
pub(crate) use registry::Registry;
mod tls;

#[cfg(test)]
mod tests;
