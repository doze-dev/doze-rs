//! Unix: mode 0700; clone with `cp` (macOS: `-c` uses clonefile; Linux: `--reflink=auto`).

use std::ffi::OsStr;
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub(super) fn make_private(path: &Path) -> io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

pub(super) fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // A new file is created 0600; an existing one keeps its mode on open, so narrow it before writing.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    f.write_all(bytes)
}

#[cfg(target_os = "macos")]
pub(super) fn clone_dir(src: &Path, dst: &Path) -> io::Result<()> {
    // clonefile shares blocks on APFS; on a volume without it, fall back to a plain recursive copy.
    super::run_cp(&[
        OsStr::new("-c"),
        OsStr::new("-R"),
        src.as_os_str(),
        dst.as_os_str(),
    ])
    .or_else(|_| super::run_cp(&[OsStr::new("-R"), src.as_os_str(), dst.as_os_str()]))
}

#[cfg(not(target_os = "macos"))]
pub(super) fn clone_dir(src: &Path, dst: &Path) -> io::Result<()> {
    super::run_cp(&[
        OsStr::new("-a"),
        OsStr::new("--reflink=auto"),
        src.as_os_str(),
        dst.as_os_str(),
    ])
}
