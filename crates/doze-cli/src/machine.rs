//! The machine the privileged commands (`dns-setup`, `trust`, `doctor --fix`) set up: what is checked
//! ([`doze_doctor::Machine`]), how its one script is run as root, and the network daemon the setup installs, which is
//! this same program started with [`doze_names::HELPER_ARG`].
//!
//! The script is run by the [`crate::Host`]: the real process asks sudo on the terminal, or the system's administrator
//! prompt when the app asked for it ([`PROMPT_ENV`]); a test's host records it and runs nothing.

use crate::ctx::{CANCELLED, Ctx, PROMPT_ENV, Refused};
use crate::diag::{Diagnostic, ids};
use doze_doctor::Machine;
use doze_names::setup::{Daemon, How, Options, Os, Plan, Setup, Shell, setup_user};
use std::io;

/// The machine as this run sees it: its system files (the host's), the doze home, this program as the daemon.
pub(crate) fn machine(ctx: &Ctx<'_>) -> Machine {
    let program = ctx
        .host
        .exe()
        .unwrap_or_else(|| std::path::PathBuf::from("doze"));
    Machine {
        setup: Setup {
            os: Os::native(),
            paths: ctx.host.setup_paths(),
            home: ctx.home.clone(),
            daemon: Daemon {
                program,
                args: vec![doze_names::HELPER_ARG.to_owned()],
            },
            user: setup_user().ok(),
        },
        zone_off: doze_doctor::zone_off(ctx.var(doze_names::ENV_ZONE).as_deref()),
    }
}

/// Whether the person asked for the system's prompt instead of sudo (the app does, it has no terminal).
pub(crate) fn prompt(ctx: &Ctx<'_>) -> bool {
    ctx.var(PROMPT_ENV).as_deref() == Some("1")
}

/// A runner for `doze_names` that hands the plan to the host.
pub(crate) fn runner<'a>(ctx: &'a Ctx<'_>) -> impl FnMut(&Plan) -> io::Result<()> + 'a {
    let ask = prompt(ctx);
    move |plan: &Plan| {
        ctx.host
            .privileged(plan, ask)
            .map_err(|r| io::Error::other(r.to_string()))
    }
}

/// What a failed setup run says: a cancelled prompt in the words the app looks for, a failed script with what it was
/// for.
pub(crate) fn failed(e: &doze_names::Error) -> Diagnostic {
    let text = e.to_string();
    if text.contains(CANCELLED) {
        return Diagnostic::new(ids::PROMPT_CANCELLED, CANCELLED)
            .note("nothing more was changed; run the command again to finish");
    }
    if text.contains("no way to ask for an administrator") {
        return Diagnostic::new(ids::NO_PROMPT, text).fix("run it in a terminal");
    }
    match e {
        doze_names::Error::Privileged { what, why } => Diagnostic::new(
            ids::PRIVILEGED_FAILED,
            format!("the privileged step failed ({what}): {why}"),
        ),
        _ => Diagnostic::new(ids::SETUP_FAILED, text),
    }
}

/// `--check` found something missing: exit 1 with the command that sets it up.
pub(crate) fn missing(what: &str, fix: &str) -> Diagnostic {
    Diagnostic::new(ids::CHECKS_FAILED, format!("{what} not set up"))
        .fix(format!("{fix} (one sudo)"))
}

/// Runs `plan` as root on the real machine (the real process's [`crate::Host::privileged`]).
pub(crate) fn run_as_root(plan: &Plan, prompt: bool) -> Result<(), Refused> {
    if prompt && plan.shell == Shell::Sh && !doze_os::privilege::is_privileged().unwrap_or(false) {
        use doze_os::privilege::{AdminError, admin, admin_supported};
        if admin_supported() {
            return admin(&plan.what, &plan.script).map_err(|e| match e {
                AdminError::Cancelled => Refused::Cancelled,
                AdminError::Unsupported => Refused::Unsupported,
                AdminError::Failed(why) => Refused::Failed(why),
            });
        }
    }
    // A terminal's sudo, the process's own shell when it is root already, UAC on Windows.
    let mut err = io::stderr();
    let how = if prompt { How::Prompt } else { How::Terminal };
    let mut o = Options { out: &mut err, how };
    doze_names::setup::run_plan(&mut o, plan).map_err(|e| match e {
        doze_names::Error::Privileged { why, .. } => Refused::Failed(why.to_string()),
        other => Refused::Failed(other.to_string()),
    })
}

/// The network daemon, as the job setup wrote starts it (`doze __doze-netd <registry> [<uid> <gid>]`): on macOS as
/// root with the person's uid and gid to drop to, on Linux as the person. It must run before anything else in the
/// process (no logging, no runtime), as Go's `names.Helper()`.
pub(crate) fn netd(args: &[std::ffi::OsString]) -> u8 {
    let args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let result = match args.as_slice() {
        [registry, uid, gid] => match (uid.parse(), gid.parse()) {
            (Ok(uid), Ok(gid)) => doze_names::netd::run_translating(registry.as_ref(), uid, gid),
            _ => return usage(),
        },
        [registry] => doze_names::netd::run_zone(registry.as_ref()),
        _ => return usage(),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("doze network daemon: {e}");
            1
        }
    }
}

fn usage() -> u8 {
    eprintln!(
        "usage: doze {} <registry> [<uid> <gid>] (started by the job doze dns-setup installs)",
        doze_names::HELPER_ARG
    );
    2
}
