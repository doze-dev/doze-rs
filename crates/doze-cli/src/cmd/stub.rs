//! Commands that need what the Rust port has not built yet. Each says so, names what it is waiting for, and exits 1
//! (`failed`, DZ0001): never a fake success, never a dry run of a plan it cannot make.

use crate::diag::{Diagnostic, ids};

/// What a command is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Needs {
    /// It talks to the doze daemon (on a system where the daemon has no control socket yet: Windows, core plan W).
    Daemon,
}

impl Needs {
    fn why(self) -> &'static str {
        match self {
            Needs::Daemon => "it talks to the doze daemon, which comes with the Rust core",
        }
    }
}

/// The diagnostic for `command` (its path, `doze up`).
pub(crate) fn not_built(command: &str, needs: Needs) -> Diagnostic {
    Diagnostic::new(
        ids::NOT_BUILT,
        format!("`{command}` is not built yet: {}", needs.why()),
    )
    .fix("the Go doze does this today")
}
