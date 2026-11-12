//! What the console may see that the protocol does not say (feature `inspect`).
//!
//! The console reads and writes the broker through [`Broker::handle`], as a client does. This module holds the few
//! facts that no request answers: when the run began, what the last stop vouched for, how many partitions are open.
//! It is read-only by construction: every method takes `&self`, builds an owned value and returns it, and the view
//! holds no writer. Each method takes the caller and checks the right the protocol would have asked for, so a
//! fact is no more visible here than it would be over Kafka's own API. Nothing is kept between calls: an idle
//! broker pays nothing for the console's existence.

use kafka_auth::authz::{CLUSTER_NAME, operation, resource};
use kafka_storage::fs::Fs;

use crate::{Broker, Caller};

mod config_help;
mod group_detail;
mod process;

pub use config_help::{ConfigHelp, Limits};
pub use group_detail::{GroupDetail, GroupKind, MemberDetail, OffsetExpiry, RebalanceDetail};
mod config_rules;
mod pace;

pub use config_rules::ConfigRule;
pub use pace::Pace;
mod partition_files;
mod settings;

pub use partition_files::PartitionFiles;
pub use process::Process;
pub use settings::{AccessFacts, DataSize, Intervals, MOST_ENTRIES};

/// What the last stop left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastStop {
    /// There was no earlier run: the data directory is new.
    None,
    /// The last stop was clean, so every partition it covered was trusted on start.
    Clean,
    /// The last stop did not vouch for the data (a crash, a kill, a power cut): every partition was checked.
    Unclean,
}

/// Facts about this run of the broker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// When this run began, in milliseconds since the epoch.
    pub started_ms: i64,
    /// What the last stop left.
    pub last_stop: LastStop,
    /// Partitions with their files open now.
    pub open_partitions: usize,
    /// The most partitions kept open at once; 0 is no bound.
    pub max_open_partitions: usize,
    /// The most files the open partitions may hold between them; 0 is no bound of its own.
    pub max_open_files: usize,
    /// The data directory, as configured.
    pub data_dir: std::path::PathBuf,
    /// How many ACLs are stored.
    pub acls: usize,
    /// What a resource that no ACL covers allows.
    pub acl_default_allow: bool,
    /// How many users have a stored SCRAM credential.
    pub stored_users: usize,
    /// What the process itself can say (memory, threads, open files), where the platform lets it be read.
    pub process: Process,
}

/// The read-only view of one broker.
#[derive(Debug)]
pub struct ConsoleView<'a, F: Fs> {
    broker: &'a Broker<F>,
}

impl<F: Fs> Broker<F> {
    /// The read-only view the console uses for what the protocol hides.
    #[must_use]
    pub fn console_view(&self) -> ConsoleView<'_, F> {
        ConsoleView { broker: self }
    }
}

impl<F: Fs> ConsoleView<'_, F> {
    /// The facts of this run, if `caller` may DESCRIBE the cluster; `None` otherwise.
    #[must_use]
    pub fn instance(&self, caller: &Caller) -> Option<Instance> {
        let b = self.broker;
        if !b
            .authz
            .allows(caller, resource::CLUSTER, CLUSTER_NAME, operation::DESCRIBE)
        {
            return None;
        }
        let last_stop = if b.first_start {
            LastStop::None
        } else if b.stop_was_clean() {
            LastStop::Clean
        } else {
            LastStop::Unclean
        };
        Some(Instance {
            started_ms: b.started_ms,
            last_stop,
            open_partitions: b.partitions.open_count(),
            max_open_partitions: b.config.max_open_partitions,
            max_open_files: b.config.max_open_descriptors,
            data_dir: b.config.data_dir.clone(),
            acls: b.authz.snapshot().len(),
            acl_default_allow: b.config.acl_default_allow,
            stored_users: b.users.stored().user_count(),
            process: Process::read(),
        })
    }
}

#[cfg(test)]
mod tests;
