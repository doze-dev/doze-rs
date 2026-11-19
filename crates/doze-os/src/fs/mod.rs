//! Files and directories: exclusive locks between processes, directories only the user can open, and cloning a data
//! directory (copy-on-write where the filesystem can) for engine templates.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

/// An exclusive lock on a file, held until dropped (or the process exits, however it exits).
#[derive(Debug)]
pub struct Lock(#[allow(dead_code)] File);

fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
}

/// Takes the lock at `path` (creating the file), waiting while another process holds it.
pub fn lock(path: &Path) -> io::Result<Lock> {
    let f = open_lock_file(path)?;
    f.lock()?;
    Ok(Lock(f))
}

/// Takes the lock at `path` if nobody holds it; `Ok(None)` if another process does.
pub fn try_lock(path: &Path) -> io::Result<Option<Lock>> {
    let f = open_lock_file(path)?;
    match f.try_lock() {
        Ok(()) => Ok(Some(Lock(f))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Creates `path` (and its parents) and makes `path` itself open to this user only: mode 0700 on Unix; on Windows a
/// protected ACL (nothing inherited) granting only this user and SYSTEM. An existing directory is narrowed too.
pub fn private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    imp::make_private(path)
}

/// Writes `bytes` to `path` (replacing it) so that only this user can read it, for an engine config that carries a
/// password: mode 0600 on Unix, the file made private before anything is written to it on Windows. An existing file is
/// narrowed too.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    imp::write_private(path, bytes)
}

/// Writes `bytes` to `path` as a program this user can run (mode 0755 on Unix; Windows has no such bit, so it is a plain
/// write): a script that stands in for a program, as the desktop app's fixtures put a stand-in `doze` in place.
pub fn write_executable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(path, bytes)?;
    imp::make_executable(path)
}

/// Makes `dst` a copy of the directory `src`: copy-on-write where the filesystem supports it (APFS clonefile, btrfs
/// and XFS reflinks), a plain copy otherwise. `dst` must not exist; its parent must.
pub fn clone_dir(src: &Path, dst: &Path) -> io::Result<()> {
    if dst.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", dst.display()),
        ));
    }
    if !src.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} is not a directory", src.display()),
        ));
    }
    imp::clone_dir(src, dst)
}

/// Clears the group and other write bits of `path` and, for a folder, of everything under it (links are left alone),
/// so nobody but this user can change an unpacked binary in place. A no-op on Windows, where the profile's ACL already
/// keeps other users out.
pub fn drop_shared_write(path: &Path) -> io::Result<()> {
    imp::drop_shared_write(path)
}

/// The permission bits of `path` (not following a link) on Unix, setuid, setgid and sticky included; `None` on
/// Windows or when it cannot be read.
pub fn mode(path: &Path) -> Option<u32> {
    imp::mode(path)
}

/// The identity of the file at `path` on its volume: the inode on Unix, the file index on Windows. A file replaced by a
/// rename has a new one even when its time and size are the same as the old one's. `None` when it cannot be read.
pub fn file_id(path: &Path) -> Option<u64> {
    imp::file_id(path)
}

/// Runs `cp` (no shell) and turns a failure into an error carrying its message.
#[cfg(unix)]
fn run_cp(args: &[&std::ffi::OsStr]) -> io::Result<()> {
    let out = std::process::Command::new("cp")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()?;
    if out.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "cp: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    )))
}
