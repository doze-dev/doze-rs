//! Commands that need what the Rust port has not built yet. Each says so, names what it is waiting for, and exits 1
//! (`failed`, DZ0001): never a fake success, never a dry run of a plan it cannot make.

use crate::cli::Cmd;
use crate::diag::{Diagnostic, ids};

/// What a command is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Needs {
    /// It talks to the doze daemon.
    Daemon,
    /// The app.
    App,
    /// The stack's shape and logs.
    Report,
    /// Handing the daemon over to the new doze.
    Handover,
}

impl Needs {
    fn why(self) -> &'static str {
        match self {
            Needs::Daemon => "it talks to the doze daemon, which comes with the Rust core",
            Needs::App => "the app is not part of the Rust doze yet",
            Needs::Report => "it comes with a later slice of the CLI (core plan C2)",
            Needs::Handover => "handing the running daemon over to the new doze is the core's job",
        }
    }
}

/// What `cmd` waits for, or `None` if it is built.
pub(crate) fn needs(cmd: &Cmd) -> Option<Needs> {
    Some(match cmd {
        Cmd::Up(_)
        | Cmd::Down(_)
        | Cmd::Sync(_)
        | Cmd::Env(_)
        | Cmd::Run(_)
        | Cmd::Migrate(_)
        | Cmd::Url(_)
        | Cmd::Connect(_) => Needs::Daemon,
        Cmd::Open(_) => Needs::App,
        Cmd::Bug => Needs::Report,
        Cmd::Doctor(_)
        | Cmd::DnsSetup(_)
        | Cmd::Trust(_)
        | Cmd::Status(_)
        | Cmd::Wake(_)
        | Cmd::Sleep(_)
        | Cmd::Restart(_)
        | Cmd::Reset(_)
        | Cmd::Task(_)
        | Cmd::Pin(_)
        | Cmd::Off(_)
        | Cmd::On(_)
        | Cmd::Logs(_)
        | Cmd::Why(_)
        | Cmd::Lint(_)
        | Cmd::Fmt(_)
        | Cmd::Add(_)
        | Cmd::Init(_)
        | Cmd::Modules(_)
        | Cmd::Version
        | Cmd::Update(_)
        | Cmd::Completion(_)
        | Cmd::Refresh(_) => {
            return None;
        }
    })
}

/// The diagnostic for `command` (its path, `doze up`).
pub(crate) fn not_built(command: &str, needs: Needs) -> Diagnostic {
    Diagnostic::new(
        ids::NOT_BUILT,
        format!("`{command}` is not built yet: {}", needs.why()),
    )
    .fix("the Go doze does this today")
}

/// Bare `doze`: the app on a terminal, the status table when piped. Neither exists yet.
pub(crate) fn bare() -> Diagnostic {
    Diagnostic::new(
        ids::NOT_BUILT,
        "`doze` alone opens the app, or prints the status table when piped: neither is built yet",
    )
    .fix("doze --help lists the commands")
}
