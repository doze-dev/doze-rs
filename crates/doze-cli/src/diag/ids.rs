//! The stable message codes. A code never changes meaning and is never reused; a script or a search can rely on it
//! where the wording moves. Grouped by whose problem it is: 00 doze's own, 01 the command line, 02 the registry and
//! modules, 03 the project's files, 04 the machine, 05 checks, 06 updates, 07 the running
//! stack.

use crate::exit::Exit;

/// One message code and the exit class it ends a run with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Id {
    pub code: &'static str,
    pub exit: Exit,
}

const fn id(code: &'static str, exit: Exit) -> Id {
    Id { code, exit }
}

/// The command exists in the surface but needs the doze core, which the Rust port has not reached.
pub const NOT_BUILT: Id = id("DZ0001", Exit::Failed);
/// A bug in doze: says so in one line and points at `doze bug`.
pub const BUG: Id = id("DZ0002", Exit::Failed);

/// The command line is wrong (clap's own finding).
pub const USAGE: Id = id("DZ0101", Exit::Usage);
/// `--json` and `-q` together.
pub const JSON_AND_QUIET: Id = id("DZ0102", Exit::Usage);
/// `--json` on a command that has no result to give.
pub const NO_JSON: Id = id("DZ0103", Exit::Usage);

/// No registry host answered.
pub const REGISTRY_UNREACHABLE: Id = id("DZ0201", Exit::Machine);
/// The registry answered with something its key did not sign.
pub const REGISTRY_UNSIGNED: Id = id("DZ0202", Exit::Failed);
/// The registry has no module by that name.
pub const NO_SUCH_MODULE: Id = id("DZ0203", Exit::NotFound);
/// The registry answered with a file doze cannot read.
pub const REGISTRY_UNREADABLE: Id = id("DZ0204", Exit::Failed);

/// No `doze.hcl` here or in any parent.
pub const NO_CONFIG: Id = id("DZ0302", Exit::NoConfig);
/// `doze.lock` cannot be read (a merge conflict, a hand edit).
pub const LOCK_UNREADABLE: Id = id("DZ0303", Exit::ConfigInvalid);

/// The doze home cannot be used.
pub const HOME_UNUSABLE: Id = id("DZ0401", Exit::Machine);
/// The browser could not be opened.
pub const NO_BROWSER: Id = id("DZ0402", Exit::Machine);

/// `doctor` found something wrong.
pub const CHECKS_FAILED: Id = id("DZ0501", Exit::Failed);
/// `modules verify` found a pin that does not match.
pub const VERIFY_MISMATCH: Id = id("DZ0502", Exit::Failed);
/// Done, but some of it could not be checked (rule 3).
pub const NOT_ALL_CHECKED: Id = id("DZ0503", Exit::Partial);

/// The newest doze is not known: the release file could not be fetched.
pub const LATEST_UNKNOWN: Id = id("DZ0601", Exit::Machine);

/// The command needs a running stack and doze is not running it.
pub const DAEMON_DOWN: Id = id("DZ0701", Exit::DaemonDown);
/// The stack declares no service (or task) by that name.
pub const NO_SUCH_SERVICE: Id = id("DZ0702", Exit::NotFound);
/// The daemon tried and it did not work: a wake, a restart, a task (its words follow).
pub const DAEMON_REFUSED: Id = id("DZ0703", Exit::Failed);
/// It took longer than `--timeout` (or the daemon's own limit).
pub const TOO_SLOW: Id = id("DZ0704", Exit::Timeout);
/// It deletes data, and there is no terminal to ask: `-y` says yes.
pub const NEEDS_YES: Id = id("DZ0705", Exit::NeedsConfirmation);
/// The daemon is there but could not be talked to: another protocol, a broken connection.
pub const DAEMON_UNREADABLE: Id = id("DZ0706", Exit::Failed);
/// Ctrl-C or SIGTERM.
pub const INTERRUPTED: Id = id("DZ0707", Exit::Interrupted);
/// Two flags that ask for different things.
pub const FLAGS_DISAGREE: Id = id("DZ0104", Exit::Usage);
/// `--run` names a run that is not kept.
pub const NO_SUCH_RUN: Id = id("DZ0708", Exit::Failed);
/// There is nothing for the command to act on (Go exits 1 here too).
pub const NOTHING_TO_DO: Id = id("DZ0709", Exit::Failed);
