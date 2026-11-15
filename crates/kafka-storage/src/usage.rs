//! What a data directory holds, by kind, read from its files alone: the partition directories (`<topic>-<n>`) with the
//! bytes of everything in them, the metadata database, and whatever else is there. Nothing is opened for writing and
//! nothing is kept, so it can look at a directory a running broker is using (the files may change under it; each number is
//! what was there when it was looked at).

use std::io;
use std::path::Path;

use crate::fs::Fs;
use crate::segment::base_of_log;

/// The most directory entries one walk looks at: a data directory of a few thousand partitions is well inside it.
pub const MOST_ENTRIES: u64 = 200_000;

/// The metadata database's file names begin with this (the database, and what redb or a crash leaves beside it).
const META: &str = "meta.redb";

/// One partition directory and what is in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionDir {
    /// The topic its name says it belongs to (whether or not the broker has such a topic).
    pub topic: String,
    pub partition: i32,
    /// Every file in it: segments, their indexes, and the small records beside them.
    pub bytes: u64,
    pub files: u64,
    /// How many segments (log files) it has.
    pub segments: usize,
    /// When its newest segment was last written, in milliseconds since the epoch.
    pub last_write_ms: Option<i64>,
}

/// A data directory, by kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirUsage {
    /// The partition directories, by topic name, then number.
    pub partitions: Vec<PartitionDir>,
    /// The metadata database (topics, committed offsets, transactions, ACLs, users, schemas).
    pub meta_bytes: u64,
    /// Everything else: the lock file, the record of the last stop, files nobody here made.
    pub other_bytes: u64,
    pub other_files: u64,
    /// The walk stopped at [`MOST_ENTRIES`]: every number is at least what it says.
    pub cut: bool,
}

impl DirUsage {
    /// Every byte found.
    #[must_use]
    pub fn total(&self) -> u64 {
        let partitions: u64 = self.partitions.iter().map(|p| p.bytes).sum();
        partitions + self.meta_bytes + self.other_bytes
    }
}

/// The topic and number a directory's name says, if it is a partition's: `<topic>-<digits>`.
#[must_use]
pub fn partition_of(name: &str) -> Option<(&str, i32)> {
    let (topic, n) = name.rsplit_once('-')?;
    if topic.is_empty() || n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((topic, n.parse().ok()?))
}

/// Looks at everything under `data` (a directory that is not there holds nothing). Symbolic links are not followed.
///
/// # Errors
/// The data directory itself cannot be listed. A file or directory under it that goes away, or cannot be looked at, while
/// the walk runs is left out.
pub fn walk_data_dir<F: Fs>(fs: &F, data: &Path) -> io::Result<DirUsage> {
    let entries = match fs.list_dir(data) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(DirUsage::default()),
        Err(e) => return Err(e),
    };
    let mut out = DirUsage::default();
    let mut seen = 0_u64;
    for entry in entries {
        if seen >= MOST_ENTRIES {
            out.cut = true;
            break;
        }
        seen += 1;
        let path = data.join(&entry.name);
        match (entry.is_dir, partition_of(&entry.name)) {
            (true, Some((topic, partition))) => {
                let mut dir = PartitionDir {
                    topic: topic.to_owned(),
                    partition,
                    bytes: 0,
                    files: 0,
                    segments: 0,
                    last_write_ms: None,
                };
                look_inside(fs, &path, &mut seen, &mut dir);
                out.partitions.push(dir);
            }
            (true, None) => {
                let (bytes, files) = sum_tree(fs, &path, &mut seen);
                out.other_bytes += bytes;
                out.other_files += files;
            }
            (false, _) => {
                let Ok(stat) = fs.stat(&path) else { continue };
                if entry.name.starts_with(META) {
                    out.meta_bytes += stat.bytes;
                } else {
                    out.other_bytes += stat.bytes;
                    out.other_files += 1;
                }
            }
        }
        out.cut |= seen > MOST_ENTRIES;
    }
    out.partitions
        .sort_by(|a, b| a.topic.cmp(&b.topic).then(a.partition.cmp(&b.partition)));
    Ok(out)
}

/// The bytes of every file under `dir` (a directory that is not there holds none): what removing it would free.
#[must_use]
pub fn dir_bytes<F: Fs>(fs: &F, dir: &Path) -> u64 {
    let mut seen = 0;
    sum_tree(fs, dir, &mut seen).0
}

/// The files of one partition directory.
fn look_inside<F: Fs>(fs: &F, dir: &Path, seen: &mut u64, into: &mut PartitionDir) {
    let Ok(entries) = fs.list_dir(dir) else {
        return;
    };
    for entry in entries {
        *seen += 1;
        let path = dir.join(&entry.name);
        if entry.is_dir {
            let (bytes, files) = sum_tree(fs, &path, seen);
            into.bytes += bytes;
            into.files += files;
            continue;
        }
        let Ok(stat) = fs.stat(&path) else { continue };
        into.bytes += stat.bytes;
        into.files += 1;
        if base_of_log(&entry.name).is_some() {
            into.segments += 1;
            into.last_write_ms = into.last_write_ms.max(Some(stat.modified_ms));
        }
    }
}

/// The bytes and count of every file under `dir`, at any depth, within the walk's bound.
fn sum_tree<F: Fs>(fs: &F, dir: &Path, seen: &mut u64) -> (u64, u64) {
    let (mut bytes, mut files) = (0, 0);
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = fs.list_dir(&next) else {
            continue;
        };
        for entry in entries {
            *seen += 1;
            if *seen > MOST_ENTRIES {
                return (bytes, files);
            }
            let path = next.join(&entry.name);
            if entry.is_dir {
                pending.push(path);
            } else if let Ok(stat) = fs.stat(&path) {
                bytes += stat.bytes;
                files += 1;
            }
        }
    }
    (bytes, files)
}

#[cfg(test)]
mod tests;
