//! What a service keeps on disk, and how a person gets it back: `doze-aws data` and the console's storage page ask every service
//! through this. Not an AWS API. A service that keeps nothing worth naming resource by resource (IAM's definitions are a few KB)
//! still answers [`Storage::reset`], so that "start again" reaches it.
//!
//! The rule every implementation keeps: what it reports is what a delete gives back. A resource's `bytes` are the bytes its rows
//! and files hold; when the deletes are done the service compacts its database files, so the disk really gets them back (redb
//! reuses a freed page but never shrinks a file by itself; a person who emptied a bucket through the S3 API and saw no space come
//! back is the case this module exists for).

use crate::service::BoxFuture;

/// One resource that holds data: a queue, a bucket, a table, a stream, a log group.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resource {
    /// The region it belongs to.
    pub region: String,
    /// Its name as the person typed it (a queue's name, a bucket's, a log group's).
    pub name: String,
    /// The bytes of data it holds: its rows' keys and values and the files it owns (an object's body, a function's code).
    pub bytes: u64,
    /// How many of its units it holds (messages, objects, items, records, events, executions).
    pub items: u64,
    /// What a part of it is that someone may not know is there: old object versions, events no retention will remove, a
    /// dead-letter queue nobody reads.
    pub notes: Vec<Note>,
}

/// A part of a resource's data worth pointing at, and what removes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Note {
    /// The bytes it holds (0 when it is a remark about growth, not a part).
    pub bytes: u64,
    /// What it is, as a sentence fragment: `in 3,120 old versions`, `no retention set: kept forever`.
    pub text: String,
    /// The [`Prune`] that removes just it, when there is one.
    pub prune: Option<Prune>,
}

/// One database file of the service, with the space a compaction would give back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct File {
    /// The region it belongs to.
    pub region: String,
    /// Its size on disk.
    pub bytes: u64,
    /// What a compaction would give back.
    pub reclaimable: u64,
}

/// Everything a service holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Its resources that hold data, in every region it has on disk.
    pub resources: Vec<Resource>,
    /// Its database files.
    pub files: Vec<File>,
}

/// Which part of a resource to remove. The default removes all its data and keeps the resource itself, with its settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prune {
    /// Only what was written before this time (milliseconds since the epoch).
    pub before_ms: Option<i64>,
    /// Only keys under this prefix (S3).
    pub prefix: Option<String>,
    /// Only the versions that are not current, and delete markers (a versioned S3 bucket).
    pub old_versions: bool,
    /// Only what nothing uses (Lambda code no function or version points at). The name is ignored.
    pub unused: bool,
    /// Count what would go, and remove nothing.
    pub dry_run: bool,
}

/// What a removal did (or, dry, would do).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Emptied {
    /// The units removed.
    pub items: u64,
    /// The bytes of data they held.
    pub bytes: u64,
    /// What the disk got back once the files were compacted (0 on a dry run).
    pub freed: u64,
}

impl Emptied {
    /// Adds another removal's counts.
    pub fn add(&mut self, other: Self) {
        self.items += other.items;
        self.bytes += other.bytes;
        self.freed += other.freed;
    }
}

/// Why a removal did nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageError {
    /// No resource of that name in that region.
    NotFound,
    /// The resource cannot be pruned that way; the text says which ways it can.
    Unsupported(String),
    /// The service failed; the text says how.
    Failed(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("not found"),
            Self::Unsupported(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

/// A service's data, for a person to see and remove.
pub trait Storage: Send + Sync {
    /// What its resources are called, singular and plural (`("queue", "queues")`), and what their units are (`"messages"`).
    fn names(&self) -> Names;

    /// Everything it holds, in every region it has on disk. Wakes the service (and each region) if it was asleep.
    fn usage(&self) -> BoxFuture<'_, Result<Usage, String>>;

    /// Removes the data of one resource, or the part [`Prune`] names, keeps the resource, and compacts.
    fn empty<'a>(
        &'a self,
        region: &'a str,
        name: &'a str,
        prune: &'a Prune,
    ) -> BoxFuture<'a, Result<Emptied, StorageError>>;

    /// Removes everything the service holds in every region, resources and definitions too, and compacts: as if it had never run.
    fn reset(&self, dry_run: bool) -> BoxFuture<'_, Result<Emptied, String>>;

    /// Gives the space deletes left in its files back to the disk; answers the bytes freed.
    fn compact(&self) -> BoxFuture<'_, Result<u64, String>>;
}

/// How a service's resources are named to a person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Names {
    /// One resource: `queue`.
    pub one: &'static str,
    /// Several: `queues`.
    pub many: &'static str,
    /// What a resource holds: `messages`.
    pub units: &'static str,
    /// The [`Prune`] fields its `empty` honours besides the default: `["before"]`, `["before", "prefix", "old-versions"]`.
    pub prunes: &'static [&'static str],
}
