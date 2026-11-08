//! The command line.
//!
//! The flags that matter are here. The ones of parts that come in later phases (groups, ACLs, the console) are
//! accepted and ignored, hidden from the help, so that the conformance harness, which passes them all, can run the
//! scenarios of the parts that exist. A flag that would make the broker safer or different in a way a user would
//! rely on (authentication, TLS) is refused instead: a broker that ignored `--sasl-user` would be open.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Parser, ValueEnum};
use kafka_broker::{Config as BrokerConfig, Defaults};
use kafka_server::Config as ServerConfig;
use kafka_wire::profile::{KafkaMajor, Profile};

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
    /// How long the first rebalance of an empty consumer group waits for more members to join (0 = not at all).
    #[arg(long, default_value = "200ms", value_parser = duration::parse)]
    group_initial_rebalance_delay: Duration,
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
    /// Log verbosity: error, warn, info, debug or trace (RUST_LOG, if set, wins).
    #[arg(long, default_value = "info")]
    pub(crate) log_level: String,
    /// Log format.
    #[arg(long, value_enum, default_value = "text")]
    pub(crate) log_format: LogFormat,

    // Accepted and ignored until the phase that has them.
    #[arg(long, hide = true)]
    meta_db: Option<String>,
    #[arg(long, hide = true)]
    consumer_session_timeout: Option<String>,
    #[arg(long, hide = true)]
    offsets_retention: Option<String>,
    #[arg(long, hide = true)]
    acl_default: Option<String>,
    #[arg(long, hide = true, value_delimiter = ',')]
    acl_super_user: Vec<String>,
    #[arg(long, hide = true, num_args = 0..=1, default_missing_value = "true")]
    console: Option<String>,
    #[arg(long, hide = true)]
    console_addr: Option<String>,

    // Refused: a broker that ignored them would be open.
    #[arg(long, hide = true, value_delimiter = ',')]
    sasl_mechanisms: Vec<String>,
    #[arg(long, hide = true)]
    sasl_user: Vec<String>,
    #[arg(long, hide = true)]
    tls_cert: Option<String>,
    #[arg(long, hide = true)]
    tls_key: Option<String>,
}

/// What the command line comes to once it has been checked.
#[derive(Debug)]
pub(crate) struct Settings {
    pub(crate) broker: BrokerConfig,
    pub(crate) server: ServerConfig,
}

impl Args {
    /// Checks the arguments and turns them into the configuration of the broker and the server.
    pub(crate) fn settings(&self) -> Result<Settings> {
        if !self.sasl_mechanisms.is_empty() || !self.sasl_user.is_empty() {
            bail!(
                "SASL is not supported yet in this edition; the broker would accept any client, so it will not start with --sasl-mechanisms or --sasl-user"
            );
        }
        if self.tls_cert.is_some() || self.tls_key.is_some() {
            bail!("TLS is not supported yet in this edition");
        }
        let Some(major) = KafkaMajor::new(self.kafka_version) else {
            bail!(
                "--kafka-version must be 1, 2, 3 or 4, not {}",
                self.kafka_version
            );
        };
        for (name, value) in [
            ("--segment-bytes", self.segment_bytes),
            ("--index-interval-bytes", self.index_interval_bytes),
            ("--message-max-bytes", self.message_max_bytes),
        ] {
            if value <= 0 {
                bail!("{name} must be positive, not {value}");
            }
        }
        if self.default_partitions <= 0
            || self.max_partitions <= 0
            || self.default_partitions > self.max_partitions
        {
            bail!(
                "--default-partitions ({}) must be positive and at most --max-partitions ({})",
                self.default_partitions,
                self.max_partitions
            );
        }
        if self.worker_threads == 0 {
            bail!("--worker-threads must be at least 1");
        }
        let none_is_zero = |n: i64| if n <= 0 { -1 } else { n };
        let mut broker = BrokerConfig::new(self.data_dir.clone());
        broker.auto_create_topics = self.auto_create_topics;
        broker.default_partitions = self.default_partitions;
        broker.max_partitions = self.max_partitions;
        broker.fsync_interval = self.fsync_interval;
        broker.group_initial_rebalance_delay = self.group_initial_rebalance_delay;
        broker.max_open_partitions = self.max_open_partitions;
        broker.defaults = Defaults {
            segment_bytes: self.segment_bytes,
            index_interval_bytes: self.index_interval_bytes,
            message_max_bytes: self.message_max_bytes,
            retention_bytes: none_is_zero(self.retention_bytes),
            retention_ms: none_is_zero(self.retention_ms),
        };
        let server = ServerConfig {
            listen: self.listen,
            advertised_host: self.advertised_host.clone(),
            advertised_port: self.advertised_port,
            profile: Profile::new(major),
            ..ServerConfig::default()
        };
        Ok(Settings { broker, server })
    }

    /// The flags of later parts that were given a value and are being ignored, for a warning.
    pub(crate) fn ignored(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.console.as_deref().is_some_and(|v| v != "false") {
            out.push("--console");
        }
        out
    }
}

#[cfg(test)]
mod tests;
