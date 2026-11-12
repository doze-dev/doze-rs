//! What the Settings and Access pages need that no request answers (feature `inspect`): the intervals the broker was
//! started with, who may be told which principals are super users and which users came from flags, and how big the data
//! directory is.
//!
//! Nothing here holds a secret: a user is a name, never a password, salt, iteration count or key. Every method takes the
//! caller and checks the right the protocol would ask for (DESCRIBE or DESCRIBE_CONFIGS on the cluster).

use std::path::Path;
use std::time::Duration;

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;

use super::ConsoleView;
use crate::Caller;

/// How often the broker looks at things on its own, as it was started (the flags' values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intervals {
    /// `--log-retention-check-interval`.
    pub retention_check: Duration,
    /// `--log-cleaner-interval`.
    pub cleaner: Duration,
    /// `--log-cleaner-enable`.
    pub cleaner_on: bool,
    /// `--fsync-interval`.
    pub fsync: Duration,
    /// `--offsets-retention`.
    pub offsets_retention: Duration,
    /// `--group-initial-rebalance-delay`.
    pub group_initial_rebalance: Duration,
    /// `--consumer-session-timeout`.
    pub consumer_session: Duration,
    /// How often open transactions are looked at: no flag sets it.
    pub transactions: Duration,
    /// This broker's id (the resource name its own dynamic configs go under).
    pub node_id: i32,
}

/// Who is who on the Access page, by name only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessFacts {
    /// What a resource that no ACL covers allows.
    pub acl_default_allow: bool,
    /// The principals no ACL refuses (`--acl-super-user`), each as `Type:name`, in order, without the console's own.
    pub super_users: Vec<String>,
    /// The names of the users configured at start (flags, a file, the environment), in order.
    pub flag_users: Vec<String>,
    /// How many users have a stored credential (made over the API).
    pub stored_users: usize,
}

/// The size of the data directory, counted by walking it (bounded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataSize {
    /// The bytes of every file found.
    pub bytes: u64,
    /// How many files.
    pub files: u64,
    /// The walk was cut at [`MOST_ENTRIES`] entries: the size is at least this.
    pub cut: bool,
}

/// The most directory entries one walk looks at: a data directory of a few thousand partitions is well inside it.
pub const MOST_ENTRIES: u64 = 200_000;

impl<F: Fs> ConsoleView<'_, F> {
    /// The intervals, if `caller` may DESCRIBE_CONFIGS of the cluster.
    #[must_use]
    pub fn intervals(&self, caller: &Caller) -> Option<Intervals> {
        let b = self.broker;
        b.cluster_allows(caller, operation::DESCRIBE_CONFIGS)
            .then_some(Intervals {
                retention_check: b.config.log_retention_check_every,
                cleaner: b.config.log_cleaner_every,
                cleaner_on: b.config.log_cleaner_enable,
                fsync: b.config.fsync_interval,
                offsets_retention: b.config.offsets_retention,
                group_initial_rebalance: b.config.group_initial_rebalance_delay,
                consumer_session: b.config.consumer_session_timeout,
                transactions: crate::txn::CHECK_EVERY,
                node_id: b.config.node_id,
            })
    }

    /// The names the Access page says, if `caller` may DESCRIBE the cluster.
    #[must_use]
    pub fn access(&self, caller: &Caller) -> Option<AccessFacts> {
        let b = self.broker;
        if !b.cluster_allows(caller, operation::DESCRIBE) {
            return None;
        }
        let mut flag_users: Vec<String> = b
            .config
            .users
            .bootstrap()
            .names()
            .map(str::to_owned)
            .collect();
        flag_users.sort();
        Some(AccessFacts {
            acl_default_allow: b.config.acl_default_allow,
            // The console's own principal is added to the set when the broker opens (SEC-241); it is not one the operator
            // listed, and the page says it on its own line.
            super_users: b
                .config
                .super_users
                .iter()
                .filter(|p| **p != format!("User:{}", kafka_auth::CONSOLE_USER))
                .cloned()
                .collect(),
            flag_users,
            stored_users: b.users.stored().user_count(),
        })
    }

    /// The size of the data directory, if `caller` may DESCRIBE the cluster. It reads the real directory (a file system
    /// that is not one has no size to give: 0), looks at most [`MOST_ENTRIES`] entries, and keeps nothing.
    #[must_use]
    pub fn data_size(&self, caller: &Caller) -> Option<DataSize> {
        let b = self.broker;
        b.cluster_allows(caller, operation::DESCRIBE)
            .then(|| walk(&b.config.data_dir))
    }
}

/// The files under `root`, summed; symbolic links are not followed.
fn walk(root: &Path) -> DataSize {
    let mut out = DataSize {
        bytes: 0,
        files: 0,
        cut: false,
    };
    let mut seen = 0_u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MOST_ENTRIES {
                out.cut = true;
                return out;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                out.files += 1;
                out.bytes += entry.metadata().map_or(0, |m| m.len());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_walk_sums_the_files_of_every_level_and_follows_no_link() {
        let dir = std::env::temp_dir().join(format!("doze-view-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("x"), [0_u8; 10]).unwrap();
        std::fs::write(dir.join("a/y"), [0_u8; 20]).unwrap();
        std::fs::write(dir.join("a/b/z"), [0_u8; 400]).unwrap();
        let size = walk(&dir);
        assert_eq!((size.bytes, size.files, size.cut), (430, 3, false));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            walk(&dir).bytes,
            0,
            "a directory that is not there is empty"
        );
    }
}
