//! The [`Fs`] operations of a simulated disk: each passes the gate, then changes names and contents.

use std::io;
use std::path::Path;

use super::control::Op;
use super::state::{NsOp, parent};
use super::{SimFile, SimFs, absolute, io_err, name_of};
use crate::fs::{DirEntry, Fs};

impl Fs for SimFs {
    type File = SimFile;

    fn open_existing(&self, path: &Path) -> io::Result<SimFile> {
        let path = absolute(path)?;
        let mut g = self.lock();
        g.gate(Op::Read, &path).map_err(io_err)?;
        let id = *g.state.volatile.files.get(&path).ok_or_else(not_found)?;
        Ok(SimFile::new(self.clone(), id, path))
    }

    fn open_or_create(&self, path: &Path) -> io::Result<(SimFile, bool)> {
        let path = absolute(path)?;
        let mut g = self.lock();
        if let Some(id) = g.state.volatile.files.get(&path).copied() {
            g.gate(Op::Read, &path).map_err(io_err)?;
            return Ok((SimFile::new(self.clone(), id, path), false));
        }
        g.gate(Op::Create, &path).map_err(io_err)?;
        if !g.state.volatile.dirs.contains(&parent(&path)) {
            return Err(not_found());
        }
        let now = g.control.now_ms;
        let id = g.state.new_inode(now);
        g.state.volatile.files.insert(path.clone(), id);
        g.state.ns_log.push(NsOp::Create {
            path: path.clone(),
            inode: id,
        });
        Ok((SimFile::new(self.clone(), id, path), true))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (absolute(from)?, absolute(to)?);
        let mut g = self.lock();
        g.gate(Op::Rename, &from).map_err(io_err)?;
        let id = g.state.volatile.files.remove(&from).ok_or_else(not_found)?;
        g.state.volatile.files.insert(to.clone(), id);
        g.state.ns_log.push(NsOp::Rename { from, to });
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<bool> {
        let path = absolute(path)?;
        let mut g = self.lock();
        g.gate(Op::Remove, &path).map_err(io_err)?;
        let there = g.state.volatile.files.remove(&path).is_some();
        if there {
            g.state.ns_log.push(NsOp::Remove { path });
        }
        Ok(there)
    }

    fn create_dir(&self, path: &Path) -> io::Result<bool> {
        let path = absolute(path)?;
        let mut g = self.lock();
        if g.state.volatile.dirs.contains(&path) {
            return Ok(false);
        }
        g.gate(Op::Mkdir, &path).map_err(io_err)?;
        if g.state.volatile.files.contains_key(&path)
            || !g.state.volatile.dirs.contains(&parent(&path))
        {
            return Err(not_found());
        }
        g.state.volatile.dirs.insert(path.clone());
        g.state.ns_log.push(NsOp::Mkdir { path });
        Ok(true)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let path = absolute(path)?;
        let mut g = self.lock();
        g.gate(Op::Read, &path).map_err(io_err)?;
        if !g.state.volatile.dirs.contains(&path) {
            return Err(not_found());
        }
        drop(g);
        let dirs = self.dir_names(&path);
        Ok(dirs)
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        let path = absolute(path)?;
        let mut g = self.lock();
        g.gate(Op::DirSync, &path).map_err(io_err)?;
        if !g.state.volatile.dirs.contains(&path) {
            return Err(not_found());
        }
        g.state.sync_dir(&path);
        Ok(())
    }
}

impl SimFs {
    fn dir_names(&self, dir: &Path) -> Vec<DirEntry> {
        let g = self.lock();
        let ns = &g.state.volatile;
        let files = ns
            .files
            .keys()
            .filter(|p| parent(p) == dir)
            .filter_map(|p| name_of(p));
        let subdirs = ns
            .dirs
            .iter()
            .filter(|p| parent(p) == dir && p.as_path() != dir);
        files
            .map(|name| DirEntry {
                name,
                is_dir: false,
            })
            .chain(
                subdirs
                    .filter_map(|p| name_of(p))
                    .map(|name| DirEntry { name, is_dir: true }),
            )
            .collect()
    }
}

pub(super) fn not_found() -> io::Error {
    io::Error::from(io::ErrorKind::NotFound)
}
