//! Where doze keeps its state: `DOZE_HOME` if set; else `~/.doze` on macOS and Linux (where the Go doze keeps it) and
//! `%LOCALAPPDATA%\doze` on Windows (per-user, not roamed, readable only by the user by default).

use std::io;
use std::path::PathBuf;

/// The variable that overrides the doze home directory.
pub const HOME_ENV: &str = "DOZE_HOME";

/// The doze home directory (not created here).
pub fn doze_home() -> io::Result<PathBuf> {
    if let Some(h) = std::env::var_os(HOME_ENV).filter(|h| !h.is_empty()) {
        return Ok(PathBuf::from(h));
    }
    default_home()
}

#[cfg(unix)]
fn default_home() -> io::Result<PathBuf> {
    std::env::home_dir()
        .map(|h| h.join(".doze"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no home directory: set HOME or DOZE_HOME",
            )
        })
}

#[cfg(windows)]
fn default_home() -> io::Result<PathBuf> {
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|l| !l.is_empty()) {
        return Ok(PathBuf::from(local).join("doze"));
    }
    std::env::home_dir()
        .map(|h| h.join("AppData").join("Local").join("doze"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no LOCALAPPDATA or profile: set DOZE_HOME",
            )
        })
}
