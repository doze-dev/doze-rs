//! What a simulated disk holds: file contents, and names, each with what is merely in memory and what a
//! power cut would keep.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(super) type InodeId = usize;

/// A change to a file's contents that has not been flushed.
#[derive(Debug, Clone)]
pub(super) enum Pending {
    Write { pos: u64, data: Vec<u8> },
    SetLen(u64),
}

#[derive(Debug, Clone, Default)]
pub(super) struct Inode {
    /// The contents now.
    pub data: Vec<u8>,
    /// The contents a power cut is certain to keep: as of the last flush.
    pub durable: Vec<u8>,
    /// What changed since.
    pub pending: Vec<Pending>,
    pub mtime_ms: i64,
    /// A flush of this file once failed: nothing written since is ever made durable (see
    /// `Control::lose_on_failed_fsync`).
    pub frozen: bool,
}

impl Inode {
    pub(super) fn write(&mut self, pos: u64, bytes: &[u8]) {
        let (start, end) = (pos as usize, pos as usize + bytes.len());
        if self.data.len() < end {
            self.data.resize(end, 0);
        }
        if let Some(dst) = self.data.get_mut(start..end) {
            dst.copy_from_slice(bytes);
        }
        self.pending.push(Pending::Write {
            pos,
            data: bytes.to_vec(),
        });
    }

    pub(super) fn set_len(&mut self, len: u64) {
        self.data.resize(len as usize, 0);
        self.pending.push(Pending::SetLen(len));
    }

    pub(super) fn flush(&mut self) {
        if !self.frozen {
            self.durable.clone_from(&self.data);
        }
        self.pending.clear();
    }
}

/// The names: files by path, and directories.
#[derive(Debug, Clone)]
pub(super) struct Namespace {
    pub files: BTreeMap<PathBuf, InodeId>,
    pub dirs: BTreeSet<PathBuf>,
}

impl Default for Namespace {
    fn default() -> Self {
        Self {
            files: BTreeMap::new(),
            dirs: BTreeSet::from([PathBuf::from("/")]),
        }
    }
}

/// A change to a directory's entries, kept until the directory is flushed.
#[derive(Debug, Clone)]
pub(super) enum NsOp {
    Create { path: PathBuf, inode: InodeId },
    Remove { path: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
    Mkdir { path: PathBuf },
    Rmdir { path: PathBuf },
}

impl NsOp {
    /// The directory whose flush makes this change durable.
    pub(super) fn dir(&self) -> PathBuf {
        let path = match self {
            Self::Create { path, .. }
            | Self::Remove { path }
            | Self::Mkdir { path }
            | Self::Rmdir { path } => path,
            Self::Rename { to, .. } => to,
        };
        parent(path)
    }

    /// Applies the change to `ns`, skipping what no longer makes sense there (a rename of a name that did
    /// not survive, a file in a directory that did not).
    pub(super) fn apply(&self, ns: &mut Namespace) {
        match self {
            Self::Create { path, inode } => {
                if ns.dirs.contains(&parent(path)) {
                    ns.files.insert(path.clone(), *inode);
                }
            }
            Self::Remove { path } => {
                ns.files.remove(path);
            }
            Self::Rename { from, to } => {
                if let Some(inode) = ns.files.remove(from) {
                    ns.files.insert(to.clone(), inode);
                }
            }
            Self::Mkdir { path } => {
                if ns.dirs.contains(&parent(path)) {
                    ns.dirs.insert(path.clone());
                }
            }
            Self::Rmdir { path } => {
                // What an earlier removal inside it left behind goes with it: a file needs its directory.
                ns.dirs.remove(path);
                ns.files.retain(|file, _| parent(file) != *path);
            }
        }
    }
}

pub(super) fn parent(path: &Path) -> PathBuf {
    path.parent()
        .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
}

/// The whole simulated disk.
#[derive(Debug, Clone, Default)]
pub(super) struct State {
    pub inodes: Vec<Inode>,
    pub volatile: Namespace,
    pub durable: Namespace,
    /// Changes to directory entries not yet flushed, oldest first.
    pub ns_log: Vec<NsOp>,
}

impl State {
    pub(super) fn new_inode(&mut self, now_ms: i64) -> InodeId {
        self.inodes.push(Inode {
            mtime_ms: now_ms,
            ..Inode::default()
        });
        self.inodes.len() - 1
    }

    pub(super) fn total_bytes(&self) -> u64 {
        let live = self.volatile.files.values();
        live.filter_map(|id| self.inodes.get(*id))
            .map(|i| i.data.len() as u64)
            .sum()
    }

    /// Flushes a directory: its pending entry changes become durable.
    pub(super) fn sync_dir(&mut self, dir: &Path) {
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.ns_log)
            .into_iter()
            .partition(|op| op.dir() == dir);
        for op in &mine {
            op.apply(&mut self.durable);
        }
        self.ns_log = rest;
    }
}
