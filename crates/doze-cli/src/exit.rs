//! The exit codes a script branches on. The codes the Go doze ships are published (the website's CLI reference:
//! "Codes are added, never renumbered"), so they stay; `machine` (10) and `partial` (11) are the two this CLI adds.
//! `--json` carries the same class as a word (`error.code`) and the number (`error.exit`).

/// How a run ended, as a class with a stable code and word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Exit {
    /// Done.
    Ok,
    /// It failed, for a reason with no code of its own.
    Failed,
    /// An unknown command or flag, a missing argument.
    Usage,
    /// The stack (or the registry) has nothing by that name.
    NotFound,
    /// The config has a mistake; the message points at it.
    ConfigInvalid,
    /// No `doze.hcl` here or in any parent.
    NoConfig,
    /// The command needs a running stack.
    DaemonDown,
    /// It took longer than allowed.
    Timeout,
    /// It needs a yes and there is no terminal to ask.
    NeedsConfirmation,
    /// The machine's problem: the network, a port, a permission.
    Machine,
    /// Done, but not all of it could be checked or reached.
    Partial,
    /// Ctrl-C or SIGTERM.
    Interrupted,
    /// `doze run` and `doze connect` exit with their command's own code.
    Child(u8),
}

/// Every class, in the order the reference lists them, with what it means.
pub const PUBLISHED: &[(Exit, &str)] = &[
    (Exit::Ok, "it worked"),
    (
        Exit::Failed,
        "it failed, for a reason with no code of its own",
    ),
    (
        Exit::Usage,
        "the command line is wrong: an unknown command or flag, a missing argument",
    ),
    (
        Exit::NotFound,
        "the stack declares no service or task by that name",
    ),
    (
        Exit::ConfigInvalid,
        "the config has a mistake (the message points at it)",
    ),
    (
        Exit::NoConfig,
        "there is no doze.hcl here or in any parent directory",
    ),
    (Exit::DaemonDown, "the command needs a running stack"),
    (Exit::Timeout, "it took longer than allowed"),
    (
        Exit::NeedsConfirmation,
        "it needs a yes and there is no terminal to ask: pass -y",
    ),
    (
        Exit::Machine,
        "the machine's problem: the network, a port, a permission (the message names it and the fix)",
    ),
    (
        Exit::Partial,
        "done, but not all of it could be checked or reached",
    ),
    (Exit::Interrupted, "it was interrupted (Ctrl-C or SIGTERM)"),
];

impl Exit {
    /// The process exit code.
    pub fn code(self) -> u8 {
        match self {
            Exit::Ok => 0,
            Exit::Failed => 1,
            Exit::Usage => 2,
            Exit::NotFound => 4,
            Exit::ConfigInvalid => 5,
            Exit::NoConfig => 6,
            Exit::DaemonDown => 7,
            Exit::Timeout => 8,
            Exit::NeedsConfirmation => 9,
            Exit::Machine => 10,
            Exit::Partial => 11,
            Exit::Interrupted => 130,
            Exit::Child(c) => c,
        }
    }

    /// The word `--json` puts in `error.code`; empty for success.
    pub fn kind(self) -> &'static str {
        match self {
            Exit::Ok => "",
            Exit::Failed | Exit::Child(_) => "failed",
            Exit::Usage => "usage",
            Exit::NotFound => "not_found",
            Exit::ConfigInvalid => "config_invalid",
            Exit::NoConfig => "no_config",
            Exit::DaemonDown => "daemon_down",
            Exit::Timeout => "timeout",
            Exit::NeedsConfirmation => "needs_confirmation",
            Exit::Machine => "machine",
            Exit::Partial => "partial",
            Exit::Interrupted => "interrupted",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published table, as the website and the Go doze have it: a renumbering here breaks every script that
    /// branches on a code.
    #[test]
    fn the_published_codes_never_move() {
        let got: Vec<(u8, &str)> = PUBLISHED
            .iter()
            .map(|(e, _)| (e.code(), e.kind()))
            .collect();
        assert_eq!(
            got,
            [
                (0, ""),
                (1, "failed"),
                (2, "usage"),
                (4, "not_found"),
                (5, "config_invalid"),
                (6, "no_config"),
                (7, "daemon_down"),
                (8, "timeout"),
                (9, "needs_confirmation"),
                (10, "machine"),
                (11, "partial"),
                (130, "interrupted"),
            ]
        );
    }
}
