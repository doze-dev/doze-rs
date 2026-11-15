//! The Go doze daemon's journal, read from disk: every wake, sleep, crash and restart, with what caused it and how
//! long it took. The app reads it for Activity ("woke in 38 ms, because worker asked") and for Away (what happened
//! while you were gone).
//!
//! The format is the Go `internal/journal`'s: one JSON object per line in `<project state>/run/journal.jsonl` (doze-projects
//! names it), rotated to `<path>.1` when the live file would pass its cap (1 MiB by default), one rotated file
//! kept. Sequence numbers rise across rotations and daemon restarts. A line that does not decode is skipped, which is
//! how a write cut short by a crash reads; an event without a sequence number is skipped too.
//!
//! This crate only reads: the daemon is the one writer, and it appends without a lock, so a reader can see a last
//! line still being written. That line does not decode and is skipped; the next read sees it whole.

mod event;
mod gojson;
mod read;

pub use event::{Cause, Event, parse_line};
pub use read::{MAX_LINE, parse, read, read_file, rotated, since, tail};

/// A start began.
pub const WAKE: &str = "wake";
/// The start finished; `took` is the time since its wake.
pub const READY: &str = "ready";
/// Stopped on purpose.
pub const SLEEP: &str = "sleep";
/// Exited by itself; `detail` is how.
pub const CRASH: &str = "crash";
/// A crashed instance is being started again.
pub const RESTART: &str = "restart";
/// The restart budget is spent.
pub const GIVE_UP: &str = "give_up";
/// A start did not complete; `detail` is why.
pub const FAILED: &str = "failed";
/// A setup or `on_ready` command ran; `detail` names it.
pub const STEP: &str = "step";
/// Pinned awake, or unpinned; `detail` is "on" or "off".
pub const PIN: &str = "pin";

/// A client connected.
pub const BY_CONNECTION: &str = "connection";
/// An instance that depends on it started; `by` names it.
pub const BY_DEPENDENCY: &str = "dependency";
/// Someone asked: the CLI, the app, the API. Go reads a missing cause as this one.
pub const BY_COMMAND: &str = "command";
/// The restart policy.
pub const BY_RESTART: &str = "restart";
/// Nobody held it for the idle time.
pub const BY_IDLE: &str = "idle";
/// The daemon is stopping.
pub const BY_SHUTDOWN: &str = "shutdown";
/// A file it watches changed; `by` names the path.
pub const BY_WATCH: &str = "watch";

/// The size at which the Go daemon rotates the live file by default. The journal holds at most about twice this.
pub const DEFAULT_MAX_BYTES: u64 = 1 << 20;
