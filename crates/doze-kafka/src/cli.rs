//! The command line.
//!
//! The flags that matter are here. The ones of parts that come in later phases (the console) are
//! accepted and ignored, hidden from the help, so that the conformance harness, which passes them all, can run the
//! scenarios of the parts that exist. A flag that would make the broker safer or different in a way a user would
//! rely on is refused instead when it cannot be honoured: a broker that ignored `--tls-cert` would send in the
//! clear (so they need the `tls` feature, and are refused without it). SASL (PLAIN, SCRAM-SHA-256, SCRAM-SHA-512) is real (`--sasl-mechanisms`, `--sasl-user`); the passwords of `--sasl-user` are kept
//! out of every `Debug` output and of every message.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Parser, ValueEnum};
use kafka_broker::{Config as BrokerConfig, Defaults};
use kafka_server::{Config as ServerConfig, Timeouts};
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
    /// SASL mechanisms to offer, comma separated (PLAIN, SCRAM-SHA-256, SCRAM-SHA-512; a SCRAM mechanism is
    /// derived from each user's password at start). With it somebody must be able to log in: give --sasl-user,
    /// --sasl-users-file or DOZE_KAFKA_SASL_USERS, or start on a data directory that holds users made over the API.
    /// Without it, no client is asked to log in.
    #[arg(long, value_delimiter = ',')]
    sasl_mechanisms: Vec<String>,
    /// A user that may log in, as user:password (split at the first colon; repeatable). Needs
    /// --sasl-mechanisms. Passwords are ASCII and are never printed. They show in `ps`: see --sasl-users-file.
    #[arg(long, value_name = "USER:PASSWORD")]
    sasl_user: Vec<sasl::Login>,
    /// A file of users that may log in, one user:password a line (split at the first colon; blank lines and lines that
    /// start with # are skipped), so that passwords stay out of `ps`. Warns at start if group or others can read it.
    /// The variable DOZE_KAFKA_SASL_USERS (comma-separated user:password, no comma in a password) is read too. Users
    /// from these are re-applied at every start, never stored, and cannot be changed over the API.
    #[arg(long, value_name = "FILE")]
    sasl_users_file: Option<PathBuf>,
    /// How long a failed login waits before it is answered, to make guessing passwords slow (Kafka's
    /// connection.failed.authentication.delay.ms, 100ms; 0 = at once). Applies to every login that is refused.
    #[arg(long, default_value = "100ms", value_parser = duration::parse)]
    sasl_failed_login_delay: Duration,

    /// What a resource that no ACL covers allows: allow (the default, so that a broker with no ACLs refuses nothing)
    /// or deny (Kafka's own default). ACLs you create are enforced either way. With SASL on, allow means every
    /// logged-in user may do everything, including managing users and ACLs (only the credentials of --acl-super-user
    /// principals are kept from them): use deny with ACLs for a broker that is shared.
    #[arg(long, default_value = "allow")]
    acl_default: acl::AclDefault,
    /// A principal that ACLs never refuse, as Type:name such as User:admin (repeatable, or separated by commas or semicolons).
    /// Only a super user may set or delete the SCRAM credential of a super user over the API: with --acl-default allow,
    /// naming the administrators here is what stops any other login from taking their place.
    #[arg(long, value_name = "PRINCIPAL")]
    acl_super_user: Vec<String>,

    // Accepted and ignored until the phase that has them.
    #[arg(long, hide = true)]
    meta_db: Option<String>,
    #[arg(long, hide = true, num_args = 0..=1, default_missing_value = "true")]
    console: Option<String>,
    #[arg(long, hide = true)]
    console_addr: Option<String>,
    /// A host name the console answers to besides localhost and IP addresses (repeatable, or comma separated; `*`
    /// turns the check off). It guards against DNS rebinding. The console arrives with Phase 9: today the flag is
    /// checked and kept, and nothing listens for it to guard.
    #[arg(long, value_name = "HOST", value_delimiter = ',')]
    console_allow_host: Vec<String>,
    /// Accept serving the console beyond loopback although SASL or TLS is on (it has no login and is plain HTTP, so
    /// without this that start is refused). The console arrives with Phase 9: today only the start-up rule uses it.
    #[arg(long)]
    console_unprotected: bool,

    /// PEM certificate chain (leaf first) to serve TLS with. Needs --tls-key.
    #[cfg(feature = "tls")]
    #[arg(long, value_name = "FILE")]
    tls_cert: Option<PathBuf>,
    /// PEM private key of the certificate (PKCS#8, SEC1 or RSA, not encrypted). Needs --tls-cert.
    #[cfg(feature = "tls")]
    #[arg(long, value_name = "FILE")]
    tls_key: Option<PathBuf>,
    // Without the feature the two flags are accepted, hidden, to be refused with a message that names the feature.
    #[cfg(not(feature = "tls"))]
    #[arg(long, hide = true)]
    tls_cert: Option<String>,
    #[cfg(not(feature = "tls"))]
    #[arg(long, hide = true)]
    tls_key: Option<String>,
}

/// What the command line comes to once it has been checked.
#[derive(Debug)]
pub(crate) struct Settings {
    pub(crate) broker: BrokerConfig,
    pub(crate) server: ServerConfig,
    /// The console flags (nothing serves a console before Phase 9).
    pub(crate) console: Console,
    /// What the operator chose that leaves something open, to be said once at start.
    pub(crate) warnings: Vec<String>,
}

impl Args {
    /// Checks the arguments and turns them into the configuration of the broker and the server.
    pub(crate) fn settings(&self) -> Result<Settings> {
        self.settings_in(&|name| std::env::var_os(name))
    }

    /// [`Args::settings`] with the environment given, so that a test need not change the process's own.
    pub(crate) fn settings_in(
        &self,
        env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<Settings> {
        // Without the feature this refuses the flags, and a value that is `()` is all that is left of them.
        #[cfg_attr(not(feature = "tls"), allow(clippy::let_unit_value, unused_variables))]
        let tls = tls::load(self.tls_cert.as_ref(), self.tls_key.as_ref())?;
        // Both flags are set or neither (and without the feature neither is accepted).
        let tls_on = self.tls_cert.is_some();
        let Some(major) = KafkaMajor::new(self.kafka_version) else {
            bail!(
                "--kafka-version must be 1, 2, 3 or 4, not {}",
                self.kafka_version
            );
        };
        self.check_numbers()?;
        let none_is_zero = |n: i64| if n <= 0 { -1 } else { n };
        let mut broker = BrokerConfig::new(self.data_dir.clone());
        broker.auto_create_topics = self.auto_create_topics;
        broker.default_partitions = self.default_partitions;
        broker.max_partitions = self.max_partitions;
        broker.fsync_interval = self.fsync_interval;
        broker.group_initial_rebalance_delay = self.group_initial_rebalance_delay;
        broker.consumer_session_timeout = self.consumer_session_timeout;
        // A test knob no flag covers: the least time between two computations of a consumer group's assignment.
        if let Some(v) = env("DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL") {
            broker.consumer_assignment_interval = duration::parse(&v.to_string_lossy())
                .map_err(|e| anyhow::anyhow!("DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL: {e}"))?;
        }
        broker.offsets_retention = self.offsets_retention;
        broker.log_retention_check_every = self.log_retention_check_interval;
        broker.log_cleaner_every = self.log_cleaner_interval;
        broker.log_cleaner_enable = self.log_cleaner_enable;
        broker.max_open_partitions = self.max_open_partitions;
        broker.acl_default_allow = self.acl_default == acl::AclDefault::Allow;
        broker.super_users = acl::super_users(&self.acl_super_user)?;
        broker.defaults = Defaults {
            segment_bytes: self.segment_bytes,
            index_interval_bytes: self.index_interval_bytes,
            message_max_bytes: self.message_max_bytes,
            retention_bytes: none_is_zero(self.retention_bytes),
            retention_ms: none_is_zero(self.retention_ms),
        };
        let sasl = sasl::setup(&sasl::Input {
            mechanisms: &self.sasl_mechanisms,
            users: &self.sasl_user,
            users_file: self.sasl_users_file.as_deref(),
            env: sasl::users_from_env(env)?,
            data_dir: &self.data_dir,
        })?;
        broker.users = sasl.users;
        let (console, mut warnings) = self.console_rule(sasl.authenticator.is_some(), tls_on)?;
        warnings.extend(tls::key_warning(self.tls_key.as_ref()));
        let server = ServerConfig {
            sasl: sasl.authenticator,
            #[cfg(feature = "tls")]
            tls,
            listen: self.listen,
            advertised_host: self.advertised_host.clone(),
            advertised_port: self.advertised_port,
            profile: Profile::new(major),
            timeouts: Timeouts {
                failed_login: self.sasl_failed_login_delay,
                ..Timeouts::default()
            },
            ..ServerConfig::default()
        };
        Ok(Settings {
            broker,
            server,
            console,
            warnings,
        })
    }

    /// The console flags, and the start-up rule between them and what protects the broker.
    fn console_rule(&self, sasl: bool, tls: bool) -> Result<(Console, Vec<String>)> {
        console::setup(&console::Input {
            console: self.console.as_deref(),
            addr: self.console_addr.as_deref(),
            allow_hosts: &self.console_allow_host,
            unprotected: self.console_unprotected,
            sasl,
            tls,
            listen: self.listen,
            acl_allow: self.acl_default == acl::AclDefault::Allow,
        })
    }

    /// The sizes, counts and times that must be above zero.
    fn check_numbers(&self) -> Result<()> {
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
        for (name, time) in [
            ("--consumer-session-timeout", self.consumer_session_timeout),
            ("--log-cleaner-interval", self.log_cleaner_interval),
            (
                "--log-retention-check-interval",
                self.log_retention_check_interval,
            ),
        ] {
            if time.is_zero() {
                bail!("{name} must be above zero");
            }
        }
        if self.worker_threads == 0 {
            bail!("--worker-threads must be at least 1");
        }
        Ok(())
    }

    /// The flags of later parts that were given a value and are being ignored, for a warning.
    pub(crate) fn ignored(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.console.as_deref().is_some_and(|v| v != "false") {
            out.push("--console");
        }
        if !self.console_allow_host.is_empty() {
            out.push("--console-allow-host");
        }
        if self.console_unprotected {
            out.push("--console-unprotected");
        }
        out
    }
}

mod acl;
mod console;
pub(crate) use console::Console;
mod sasl;
pub(crate) use sasl::NO_USERS;
mod tls;

#[cfg(test)]
mod tests;
