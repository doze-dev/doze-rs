//! An open file on the simulated disk.

use std::io;
use std::path::PathBuf;

use super::SimFs;
use super::control::{Failure, Op};
use super::state::{Inode, InodeId};
use crate::fs::FsFile;

/// An open file of a [`SimFs`].
#[derive(Debug)]
pub struct SimFile {
    fs: SimFs,
    id: InodeId,
    /// The name it was opened under, for the trace.
    path: PathBuf,
}

impl SimFile {
    pub(super) fn new(fs: SimFs, id: InodeId, path: PathBuf) -> Self {
        Self { fs, id, path }
    }
}

fn gone() -> io::Error {
    io::Error::from(io::ErrorKind::NotFound)
}

impl FsFile for SimFile {
    fn read_at(&self, buf: &mut [u8], pos: u64) -> io::Result<usize> {
        let mut g = self.fs.lock();
        g.gate(Op::Read, &self.path).map_err(|r| r.error)?;
        let inode = g.state.inodes.get(self.id).ok_or_else(gone)?;
        let from = usize::try_from(pos).unwrap_or(usize::MAX);
        let src = inode.data.get(from..).unwrap_or_default();
        let n = src.len().min(buf.len());
        if let (Some(dst), Some(src)) = (buf.get_mut(..n), src.get(..n)) {
            dst.copy_from_slice(src);
        }
        Ok(n)
    }

    fn write_all_at(&self, data: &[u8], pos: u64) -> io::Result<()> {
        let mut g = self.fs.lock();
        g.gate(Op::Write, &self.path).map_err(|r| r.error)?;
        let total = g.state.total_bytes();
        let now = g.control.now_ms;
        let capacity = g.control.capacity;
        let inode = g.state.inodes.get_mut(self.id).ok_or_else(gone)?;
        let have = inode.data.len() as u64;
        // Room left on the disk decides how much of the write lands before it fails.
        let room = capacity.map(|c| have + c.saturating_sub(total));
        let n = match room {
            Some(max_end) => (max_end.saturating_sub(pos) as usize).min(data.len()),
            None => data.len(),
        };
        if let Some(part) = data.get(..n).filter(|p| !p.is_empty()) {
            inode.write(pos, part);
            inode.mtime_ms = now;
        }
        if n < data.len() {
            return Err(Failure::Full.error());
        }
        Ok(())
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        let mut g = self.fs.lock();
        g.gate(Op::Truncate, &self.path).map_err(|r| r.error)?;
        let now = g.control.now_ms;
        let inode = g.state.inodes.get_mut(self.id).ok_or_else(gone)?;
        inode.set_len(len);
        inode.mtime_ms = now;
        Ok(())
    }

    fn size(&self) -> io::Result<u64> {
        let g = self.fs.lock();
        g.state
            .inodes
            .get(self.id)
            .map(|i| i.data.len() as u64)
            .ok_or_else(gone)
    }

    fn sync_data(&self) -> io::Result<()> {
        let mut g = self.fs.lock();
        let lose = g.control.lose_on_failed_fsync;
        if let Err(refused) = g.gate(Op::Fsync, &self.path) {
            if let (true, Some(inode)) = (refused.injected && lose, g.state.inodes.get_mut(self.id))
            {
                freeze(inode);
            }
            return Err(refused.error);
        }
        g.state.inodes.get_mut(self.id).ok_or_else(gone)?.flush();
        Ok(())
    }

    fn modified_ms(&self) -> io::Result<i64> {
        let g = self.fs.lock();
        g.state
            .inodes
            .get(self.id)
            .map(|i| i.mtime_ms)
            .ok_or_else(gone)
    }
}

/// The kernel gave up on these pages: they stay readable and are never written.
fn freeze(inode: &mut Inode) {
    inode.frozen = true;
    inode.pending.clear();
}
