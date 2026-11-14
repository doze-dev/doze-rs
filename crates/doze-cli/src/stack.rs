//! The running stack a command talks to: the project's `doze.hcl` names a state folder under the doze home
//! (`projects/<slug>`, the slug Go gives it), and the daemon that runs the stack listens on `run/doze.admin.sock` in
//! it. The CLI is a client of that daemon, as the app is (rule 7): it asks, the daemon does.
//!
//! The config itself is not read here (that is the core's `doze-config`), so a config that moves its state with
//! `home` or `data_dir` is found at the default place, as Go's own degraded path finds it.

use crate::cmd::modules;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::Mode;
use crate::project::Project;
use doze_control::{Client, Connection, Error, InstanceView, Snapshot, fault};
use doze_projects::StackKey;
use std::time::Duration;

/// How long a read (status, logs) waits for the daemon's answer.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// This project's stack.
#[derive(Debug, Clone)]
pub(crate) struct Stack {
    pub(crate) project: Project,
    pub(crate) key: StackKey,
}

impl Stack {
    /// The stack of the project here (or at `-c`); exit 6 when there is no `doze.hcl`.
    pub(crate) fn find(ctx: &Ctx<'_>, mode: &Mode) -> Result<Stack, Diagnostic> {
        let project = modules::find(ctx, mode)?;
        let key = StackKey::for_config(&ctx.home, &project.config);
        Ok(Stack { project, key })
    }

    fn client(&self, timeout: Duration) -> Client {
        Client::socket(self.key.socket())
            .named("cli")
            .with_timeout(Some(timeout))
    }

    /// A connection to the daemon, `None` when nothing is listening. A daemon that answers but cannot be talked to
    /// (another protocol) is an error, not "down".
    pub(crate) fn open(&self, timeout: Duration) -> Result<Option<Connection>, Diagnostic> {
        match self.client(timeout).dial() {
            Ok(c) => Ok(Some(c)),
            Err(Error::Connect(_)) => Ok(None),
            Err(e) => Err(daemon_error(&e, None)),
        }
    }

    /// A connection for reading (status, logs).
    pub(crate) fn read(&self) -> Result<Option<Connection>, Diagnostic> {
        self.open(READ_TIMEOUT)
    }

    /// A connection the command cannot do without; exit 7 when doze is not running this stack.
    pub(crate) fn require(&self, timeout: Duration, what: &str) -> Result<Connection, Diagnostic> {
        self.open(timeout)?.ok_or_else(|| down(what))
    }
}

/// "doze is not running", for a command that needs it to be (exit 7).
pub(crate) fn down(what: &str) -> Diagnostic {
    Diagnostic::new(
        ids::DAEMON_DOWN,
        format!("{what} needs the stack running, and doze is not running it"),
    )
    .fix("doze up")
}

/// What the daemon said, as a diagnostic: its words, its kind as the exit class.
pub(crate) fn daemon_error(e: &Error, subject: Option<&str>) -> Diagnostic {
    let words = match subject {
        Some(s) => format!("{s}: {e}"),
        None => e.to_string(),
    };
    match e.code() {
        Some(fault::NOT_FOUND) => Diagnostic::new(ids::NO_SUCH_SERVICE, words).fix("doze status"),
        Some(fault::TIMEOUT) => Diagnostic::new(ids::TOO_SLOW, words)
            .fix("doze logs <name> says why · --timeout gives it longer"),
        Some(fault::DAEMON_DOWN) => down("this"),
        Some(fault::NEEDS_CONFIRMATION) => Diagnostic::new(ids::NEEDS_YES, words).fix("-y"),
        _ => match e {
            Error::Timeout(_) => Diagnostic::new(ids::TOO_SLOW, words)
                .note("the daemon may still finish it")
                .fix("doze status · --timeout gives it longer"),
            Error::Daemon { .. } => Diagnostic::new(ids::DAEMON_REFUSED, words),
            _ => Diagnostic::new(ids::DAEMON_UNREADABLE, words).fix("doze down, then doze up"),
        },
    }
}

/// The snapshot, or what kept the daemon from giving it.
pub(crate) fn snapshot(c: &Connection) -> Result<Snapshot, Diagnostic> {
    c.snapshot().map_err(|e| daemon_error(&e, None))
}

/// The instances the config declares, in the daemon's order. The daemon also reports instances added at run time
/// (`declared: false`); the stack's own views (status, the name checks) are about what doze.hcl says, as Go's are.
pub(crate) fn declared(snap: &Snapshot) -> impl Iterator<Item = &InstanceView> {
    snap.instances.iter().filter(|v| v.declared)
}

/// Every name is one the stack declares; exit 4 naming what it does declare otherwise.
pub(crate) fn check_names(snap: &Snapshot, names: &[String]) -> Result<(), Diagnostic> {
    let Some(missing) = names
        .iter()
        .find(|n| !declared(snap).any(|v| &v.name == *n))
    else {
        return Ok(());
    };
    Err(not_declared(snap, missing))
}

/// "the stack has no service called …", with the ones it has.
pub(crate) fn not_declared(snap: &Snapshot, name: &str) -> Diagnostic {
    let all: Vec<&str> = declared(snap).map(|v| v.name.as_str()).collect();
    let d = Diagnostic::new(
        ids::NO_SUCH_SERVICE,
        format!("the stack has no service called {name:?}"),
    );
    if all.is_empty() {
        d.note("it declares none").fix("doze add <module>")
    } else {
        d.note(format!("it has {}", all.join(", ")))
            .fix("doze status")
    }
}

/// The named ones, or with no names every one that is not turned off in doze.hcl (Go's `targetsOrAll`).
pub(crate) fn targets(snap: &Snapshot, names: &[String]) -> Result<Vec<String>, Diagnostic> {
    if names.is_empty() {
        return Ok(declared(snap)
            .filter(|v| !v.paused && v.kind != "task")
            .map(|v| v.name.clone())
            .collect());
    }
    check_names(snap, names)?;
    Ok(names.to_vec())
}
