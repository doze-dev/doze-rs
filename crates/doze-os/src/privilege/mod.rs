//! Running engines without administrator rights.
//!
//! Postgres refuses to start under an elevated Windows token ("Execution of PostgreSQL by a user with administrative
//! permissions is not permitted", W0), and initdb refuses root on Unix. So doze starts nothing privileged:
//!
//! - Windows: [`relaunch_unprivileged`] re-runs the current process with the Administrators and Power Users groups
//!   disabled (deny-only), as `pg_ctl` does, and waits for it. Everything that copy starts inherits a normal token.
//! - Unix: running as root is refused; there is no safe user to switch to.

use std::io;

mod admin;

pub use admin::{AdminError, admin, admin_supported, prompt};

#[cfg(windows)]
mod dacl;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

/// Set in the environment of a re-launched copy, so it can never re-launch again.
pub const RELAUNCHED_ENV: &str = "DOZE_UNPRIVILEGED";

/// Whether this process holds the rights engines refuse to run with: an enabled Administrators (or Power Users)
/// group on Windows, euid 0 on Unix.
pub fn is_privileged() -> io::Result<bool> {
    #[cfg(windows)]
    {
        windows::is_privileged()
    }
    #[cfg(unix)]
    {
        Ok(rustix::process::geteuid().is_root())
    }
}

/// Makes sure this process runs unprivileged. Call it first thing in `main`, before any thread starts.
///
/// - `Ok(None)`: already unprivileged; carry on.
/// - `Ok(Some(code))` (Windows only): an unprivileged copy of this process ran to completion with this exit code;
///   exit with it.
/// - `Err(_)`: privileged and nothing can be done (root on Unix, or a re-launched copy that is still privileged).
pub fn relaunch_unprivileged() -> io::Result<Option<i32>> {
    if !is_privileged()? {
        return Ok(None);
    }
    if std::env::var_os(RELAUNCHED_ENV).is_some() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "still privileged after dropping administrator rights",
        ));
    }
    #[cfg(windows)]
    {
        windows::relaunch().map(Some)
    }
    #[cfg(unix)]
    {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "doze does not run as root; run it as your own user",
        ))
    }
}

/// Gives up root for good, becoming `uid`/`gid` (supplementary groups: `gid` alone). For a daemon started as root
/// to do one privileged thing (open a tunnel, add a route) and nothing else: call it before any thread starts, since
/// the change must cover the whole process. Checked afterwards: a process that could get root back is an error.
pub fn drop_root(uid: u32, gid: u32) -> io::Result<()> {
    if uid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "drop_root: the user to become must not be root",
        ));
    }
    #[cfg(unix)]
    {
        unix::drop_root(uid, gid)
    }
    #[cfg(windows)]
    {
        let _ = gid;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "drop_root: there is no root to give up on Windows",
        ))
    }
}
