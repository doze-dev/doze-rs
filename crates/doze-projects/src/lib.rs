//! The Go doze's stacks as it leaves them on disk, read without the daemon: the record each stack keeps between runs
//! (`<home>/projects/<slug>/project.json`, schema 1), the stack keys the Go app uses (the state folder), the running
//! stacks (their control sockets), and the stale-pid check that tells a socket left behind by a dead daemon from a
//! live one.
//!
//! The record is also written ([`update`], [`save`]), byte for byte as Go writes it; the rest is read-only. The rules
//! are the Go code's (`internal/projects`, `internal/config/paths.go`, `internal/daemon`, `internal/desk/hub`);
//! tests/fixtures holds records the Go code wrote.

mod discover;
mod gojson;
mod paths;
mod rawscan;
mod record;
mod write;

pub use discover::{DaemonPid, Running, daemon_pid, running};
pub use paths::{StackKey, slug};
pub use record::{Error, FILE_NAME, Project, Raw, SCHEMA, decode, decode_bytes, list, load};
pub use write::{encode, save, update};

/// The doze home every program shares: `$DOZE_HOME`, else the platform's place (`~/.doze` on macOS and Linux).
pub fn home() -> std::io::Result<std::path::PathBuf> {
    doze_os::home::doze_home()
}
