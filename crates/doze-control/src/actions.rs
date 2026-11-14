//! The one table of verbs a person can run against a stack (Go `internal/actions`, its rows kept, plus the Rust CLI's
//! `connect` and `why`). The CLI's commands and the app's and the dash's keys and palette are projections of it, so
//! no surface can grow a verb the others lack without a test saying so (the CLI's parity test walks it both ways).
//!
//! The table holds facts: the verb, its aliases, what it takes, whether it destroys, the control op it maps to. Each
//! surface keeps its own thin adapter.

/// What a verb takes after its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg {
    None,
    /// An optional service name; none means all (or every one's, aggregated).
    Optional,
    /// A service name.
    Required,
}

/// How a verb runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One control request with this op and the service as its target.
    Op(&'static str),
    /// Surface-specific (print env, open a client, edit doze.hcl): the table documents it, the surface implements it.
    Local,
}

/// One verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    /// One line, for palette rows and help.
    pub summary: &'static str,
    pub arg: Arg,
    /// Destroys or interrupts: an interactive surface confirms first; the CLI asks or takes `-y`.
    pub confirm: bool,
    pub kind: Kind,
    /// For an op with an optional name: the daemon takes an empty target as "all".
    pub op_takes_all: bool,
    /// A CLI command.
    pub cli: bool,
    /// Offered in the dash's palette.
    pub dash: bool,
    /// The dash's key, when it has one.
    pub key: Option<&'static str>,
}

const fn row(name: &'static str, summary: &'static str, arg: Arg, kind: Kind) -> Action {
    Action {
        name,
        aliases: &[],
        summary,
        arg,
        confirm: false,
        kind,
        op_takes_all: false,
        cli: true,
        dash: false,
        key: None,
    }
}

const fn also(mut a: Action, aliases: &'static [&'static str]) -> Action {
    a.aliases = aliases;
    a
}

const fn dash(mut a: Action, key: Option<&'static str>) -> Action {
    a.dash = true;
    a.key = key;
    a
}

const fn confirm(mut a: Action) -> Action {
    a.confirm = true;
    a
}

const fn all(mut a: Action) -> Action {
    a.op_takes_all = true;
    a
}

use Arg::{None as NoArg, Optional, Required};
use Kind::{Local, Op};

/// The table, most used first (palettes and docs list it in this order).
pub const ALL: &[Action] = &[
    dash(
        also(
            row(
                "wake",
                "wake a service now (with its dependencies); no arg wakes all",
                Optional,
                Op("boot"),
            ),
            &["boot"],
        ),
        Some("w"),
    ),
    dash(
        confirm(all(also(
            row(
                "sleep",
                "put a service to sleep (dependents first); data is kept; no arg sleeps all awake",
                Optional,
                Op("down"),
            ),
            &["reap"],
        ))),
        Some("s"),
    ),
    dash(
        confirm(row(
            "restart",
            "restart a service's backend",
            Required,
            Op("restart"),
        )),
        Some("R"),
    ),
    dash(
        confirm(row(
            "reset",
            "wipe a service's data and re-provision fresh",
            Required,
            Op("reset"),
        )),
        None,
    ),
    row(
        "task",
        "run a task now, or list the tasks and how they last ran",
        Optional,
        Op("task"),
    ),
    dash(
        all(row(
            "up",
            "converge structure and wake every enabled service",
            Optional,
            Op("up"),
        )),
        None,
    ),
    dash(
        all(also(
            row(
                "sync",
                "reconcile a service with the config (create new, update changed, prune removed)",
                Optional,
                Op("apply"),
            ),
            &["apply"],
        )),
        None,
    ),
    dash(
        also(
            row(
                "pin",
                "keep a service awake past its idle timeout",
                Required,
                Op("keepawake"),
            ),
            &["keepawake"],
        ),
        Some("p"),
    ),
    dash(
        row(
            "off",
            "take a service out of this run: stop it, and nothing wakes it until on",
            Required,
            Op("off"),
        ),
        None,
    ),
    dash(
        row(
            "on",
            "put a service back into this run; it wakes on its next connection",
            Required,
            Op("on"),
        ),
        None,
    ),
    dash(
        row(
            "logs",
            "show a service's logs; no arg aggregates all",
            Optional,
            Local,
        ),
        None,
    ),
    row(
        "env",
        "show the connection env vars (DATABASE_URL, AWS_ENDPOINT_URL_*, …)",
        Optional,
        Local,
    ),
    dash(
        also(
            row("url", "copy a service's connect line", Required, Local),
            &["copy"],
        ),
        Some("y"),
    ),
    row(
        "connect",
        "open the engine's own client on a service, signed in",
        Required,
        Local,
    ),
    row(
        "why",
        "why a service is awake or asleep: what woke it, what keeps it, when it sleeps",
        Required,
        Local,
    ),
    // Headless only: the dash is the thing (status), or the verb makes no sense in a live view.
    also(
        row(
            "status",
            "show every service: state, endpoint, resource use",
            NoArg,
            Local,
        ),
        &["tree", "ls", "ps"],
    ),
    row(
        "down",
        "sleep every service and stop the daemon",
        NoArg,
        Local,
    ),
    row(
        "run",
        "ensure the daemon is up, then run a command",
        NoArg,
        Local,
    ),
    row("add", "add a module or a service to doze.hcl", NoArg, Local),
    row(
        "lint",
        "validate the config without touching anything",
        NoArg,
        Local,
    ),
    row(
        "fmt",
        "format doze.hcl and its sibling *.doze.hcl",
        NoArg,
        Local,
    ),
    row(
        "migrate",
        "move a postgres instance's data to the major doze.hcl now declares",
        Required,
        Local,
    ),
    row("init", "scaffold a doze.hcl", NoArg, Local),
    row(
        "doctor",
        "diagnose the doze environment and configuration",
        NoArg,
        Local,
    ),
    row(
        "trust",
        "set this machine up for the stack's own domains: routes and https",
        NoArg,
        Local,
    ),
];

/// The palette's actions, in order.
pub fn palette() -> impl Iterator<Item = &'static Action> {
    ALL.iter().filter(|a| a.dash)
}

/// The action a verb or alias names (any case).
pub fn lookup(verb: &str) -> Option<&'static Action> {
    let v = verb.to_lowercase();
    ALL.iter()
        .find(|a| a.name == v || a.aliases.contains(&v.as_str()))
}

/// The palette's actions whose name or an alias starts with `prefix` (all of them for an empty one).
pub fn matching(prefix: &str) -> Vec<&'static Action> {
    let p = prefix.to_lowercase();
    palette()
        .filter(|a| a.name.starts_with(&p) || a.aliases.iter().any(|al| al.starts_with(&p)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Go: TestLookupResolvesNamesAndAliases.
    #[test]
    fn names_and_aliases_resolve_in_any_case() {
        for (input, want) in [
            ("wake", "wake"),
            ("boot", "wake"),
            ("reap", "sleep"),
            ("keepawake", "pin"),
            ("WAKE", "wake"),
        ] {
            assert_eq!(lookup(input).map(|a| a.name), Some(want), "{input}");
        }
        assert!(lookup("frobnicate").is_none());
    }

    // Go: TestMatchFiltersDashActions.
    #[test]
    fn the_palette_matches_by_prefix() {
        assert!(matching("").iter().all(|a| a.dash));
        let names: Vec<&str> = matching("re").iter().map(|a| a.name).collect();
        for want in ["restart", "reset", "sleep"] {
            assert!(names.contains(&want), "{want} in {names:?}");
        }
    }

    // Go: TestOpActionsDeclareOps, and nothing is dash-only.
    #[test]
    fn ops_name_an_op_and_every_dash_action_is_a_command() {
        for a in ALL {
            if let Kind::Op(op) = a.kind {
                assert!(!op.is_empty(), "{}", a.name);
            }
            assert!(!a.dash || a.cli, "{} is dash-only", a.name);
        }
    }
}
