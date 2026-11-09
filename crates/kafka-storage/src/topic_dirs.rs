//! Where a topic's partitions live, and making and removing them.
//!
//! Partition `n` of topic `t` is the directory `<data>/<t>-<n>`. A topic name cannot hold a path separator
//! (see `TopicName`), so a name cannot name a directory outside the data directory.

use std::io;
use std::path::{Path, PathBuf};

use kafka_wire::ids::{PartitionCount, PartitionId, TopicName};

use crate::error::{Doing, StorageError};
use crate::fs::Fs;

/// The directory of one partition.
#[must_use]
pub fn partition_dir(data: &Path, topic: &TopicName, partition: PartitionId) -> PathBuf {
    data.join(format!("{topic}-{partition}"))
}

/// Makes the directory of every partition of a new topic, and flushes the data directory so that they are
/// there after a power cut. A directory that already exists is left as it is.
///
/// # Errors
/// A directory cannot be made or the flush fails. Some directories may have been made; the caller takes the
/// topic back with [`remove_topic_dirs`].
pub fn create_partition_dirs<F: Fs>(
    fs: &F,
    data: &Path,
    topic: &TopicName,
    count: PartitionCount,
) -> Result<(), StorageError> {
    create_partition_dirs_from(fs, data, topic, 0, count)
}

/// As [`create_partition_dirs`], for the partitions from number `from` up (a topic that is given more).
///
/// # Errors
/// As [`create_partition_dirs`]; the caller takes the new ones back with [`remove_partition_dirs_from`].
pub fn create_partition_dirs_from<F: Fs>(
    fs: &F,
    data: &Path,
    topic: &TopicName,
    from: usize,
    count: PartitionCount,
) -> Result<(), StorageError> {
    for partition in count.ids().skip(from) {
        fs.create_dir(&partition_dir(data, topic, partition))
            .doing("making a partition directory")?;
    }
    fs.sync_dir(data).doing("flushing the data directory")
}

/// Removes the directories of a topic's partitions with everything in them, and flushes the data directory.
/// Directories that are not there are not an error. It carries on past a failure and reports the first.
///
/// # Errors
/// The first removal or flush that failed.
pub fn remove_topic_dirs<F: Fs>(
    fs: &F,
    data: &Path,
    topic: &TopicName,
    count: PartitionCount,
) -> Result<(), StorageError> {
    remove_partition_dirs_from(fs, data, topic, 0, count)
}

/// As [`remove_topic_dirs`], for the partitions from number `from` up (taking back what a topic was given).
///
/// # Errors
/// The first removal or flush that failed.
pub fn remove_partition_dirs_from<F: Fs>(
    fs: &F,
    data: &Path,
    topic: &TopicName,
    from: usize,
    count: PartitionCount,
) -> Result<(), StorageError> {
    let mut first = None;
    for partition in count.ids().skip(from) {
        let removed = remove_dir_and_files(fs, &partition_dir(data, topic, partition));
        first = first.or(removed.err());
    }
    let flushed = fs.sync_dir(data);
    match first.or(flushed.err()) {
        None => Ok(()),
        Some(source) => Err(StorageError::Io {
            doing: "removing a topic's partitions",
            source,
        }),
    }
}

/// Removes directories named like partitions of `topic` (`<topic>-<digits>`) that no topic owns. The caller has
/// just been told the name is free, so what is there belongs to no one (a lost metadata database leaves them),
/// and a new topic must not adopt another one's records. Returns how many it removed.
///
/// # Errors
/// The listing or a removal failed.
pub fn remove_stray_dirs<F: Fs>(
    fs: &F,
    data: &Path,
    topic: &TopicName,
) -> Result<usize, StorageError> {
    let prefix = format!("{topic}-");
    let strays: Vec<_> = fs
        .list_dir(data)
        .doing("listing the data directory")?
        .into_iter()
        .filter(|e| {
            e.is_dir
                && e.name
                    .strip_prefix(&prefix)
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    for dir in &strays {
        remove_dir_and_files(fs, &data.join(&dir.name))
            .doing("removing a stray partition directory")?;
    }
    if !strays.is_empty() {
        fs.sync_dir(data).doing("flushing the data directory")?;
    }
    Ok(strays.len())
}

fn remove_dir_and_files<F: Fs>(fs: &F, dir: &Path) -> io::Result<()> {
    let entries = match fs.list_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        fs.remove_file(&dir.join(entry.name))?;
    }
    fs.remove_dir(dir).map(drop)
}

#[cfg(test)]
mod tests;
