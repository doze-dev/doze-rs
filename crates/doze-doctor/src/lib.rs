//! What doze needs from this machine, part by part, and the one privileged repair that puts it right (core plan P1,
//! section 5). Everything here is data: the checks a person reads (`doze doctor`, the app's Settings › Machine), the
//! repair's script, and what the repair says it will do are made from the same steps, so the three cannot drift.
//!
//! Nothing here asks for a password or runs a script: the caller runs the [`repair::Repair`]'s one plan its own way
//! (sudo in a terminal, the system's prompt from the app, a recorder in a test), through `doze_names::setup::run_plan`.
//!
//! - [`names`]: the `.doze` zone's machine setup (the network daemon, the resolver route), and who answers for it.
//! - [`domains`]: a stack's own domains: routed here, the authority trusted, stale routes.
//! - [`leftovers`]: what doze left behind that a person may want gone (routes nobody declares, names of dead processes).
//! - [`repair`]: the failing parts as one script under one prompt.

pub mod domains;
pub mod leftovers;
pub mod names;
pub mod repair;

use doze_names::setup::{Paths, Setup};
use serde::Serialize;

/// How one check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ok,
    /// Works, with something worth knowing.
    Warn,
    Fail,
    /// Could not be looked at; never counted as passing.
    NotChecked,
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// What was checked: `names: network`, `domains: route dev.acme.com`, `registry`. The `names: ` and `domains: `
    /// prefixes are the machine contract the app's Settings › Machine reads (Go's doctor names them so).
    pub name: String,
    pub state: State,
    /// Whether it is not failing (Go's `ok`, which the app reads). A check that could not be made is not failing, and
    /// is still not a tick: `state` says which.
    pub ok: bool,
    pub detail: String,
    /// The command that puts it right, when one does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl Check {
    pub fn new(name: impl Into<String>, state: State, detail: impl Into<String>) -> Check {
        Check {
            name: name.into(),
            state,
            ok: state != State::Fail,
            detail: detail.into(),
            fix: None,
        }
    }

    /// A check from a step that is either done or not.
    pub fn step(name: impl Into<String>, done: bool, detail: impl Into<String>) -> Check {
        Check::new(name, if done { State::Ok } else { State::Fail }, detail)
    }

    /// The command that puts it right, kept only while it is failing.
    pub fn fix(mut self, fix: &str) -> Check {
        if self.state == State::Fail {
            self.fix = Some(fix.to_owned());
        }
        self
    }
}

/// The command that sets the `.doze` names up.
pub const FIX_NAMES: &str = "doze dns-setup";
/// The command that sets a stack's domains up.
pub const FIX_DOMAINS: &str = "doze trust";
/// The command that replaces a broken authority.
pub const FIX_ROTATE: &str = "doze trust --rotate";

/// The machine being checked: its setup (system, files, the daemon setup installs) and whether the zone is on.
#[derive(Debug, Clone)]
pub struct Machine {
    pub setup: Setup,
    /// `DOZE_ZONE=off`: nothing about names is checked or set up.
    pub zone_off: bool,
}

impl Machine {
    /// The doze home whose names and domains are checked.
    pub fn home(&self) -> &std::path::Path {
        &self.setup.home
    }

    /// The system files: the real ones, or a test's.
    pub fn paths(&self) -> &Paths {
        &self.setup.paths
    }
}

/// Whether a `DOZE_ZONE` value switches the zone off (Go `names.Disabled`).
pub fn zone_off(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        )
    })
}
