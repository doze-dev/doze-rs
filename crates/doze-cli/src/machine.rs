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

/// What a refused or failed privileged run says: a cancelled prompt in the words the app looks for (exit 1), no way
/// to ask (exit 10), a failed script with what it was for (exit 10).
pub(crate) fn refused(r: &Refused, plan: &Plan) -> Diagnostic {
    match r {
        Refused::Cancelled => Diagnostic::new(ids::PROMPT_CANCELLED, CANCELLED)
            .note("nothing more was changed; run the command again to finish"),
        Refused::Unsupported => {
            Diagnostic::new(ids::NO_PROMPT, r.to_string()).fix("run it in a terminal")
        }
        Refused::Failed(why) => Diagnostic::new(
            ids::PRIVILEGED_FAILED,
            format!("the privileged step failed ({}): {why}", plan.what),
        ),
    }
}

/// What a setup step that is not the script says (reading the machine, the authority's files).
pub(crate) fn failed(e: &doze_names::Error) -> Diagnostic {
    Diagnostic::new(ids::SETUP_FAILED, e.to_string())
}

/// `--check` found something missing: exit 1 with the command that sets it up.
pub(crate) fn missing(what: &str, fix: &str) -> Diagnostic {
    Diagnostic::new(ids::CHECKS_FAILED, format!("{what} not set up"))
        .fix(format!("{fix} (one sudo)"))
}

/// Runs `plan` as root on the real machine (the real process's [`crate::Host::privileged`]): its own shell when it is
/// root already, the system's prompt when asked for and there is one (Go `adminprompt.Requested`), else sudo on the
/// terminal. What the script prints goes to stderr, so `--json`'s stdout is the envelope alone.
pub(crate) fn run_as_root(plan: &Plan, prompt: bool) -> Result<(), Refused> {
    use doze_os::privilege::{AdminError, admin, admin_supported, is_privileged};
    let root = is_privileged().unwrap_or(false);
    if plan.shell == Shell::Sh {
        if !root && prompt && admin_supported() {
            return admin(&plan.what, &plan.script).map_err(|e| match e {
                AdminError::Cancelled => Refused::Cancelled,
                AdminError::Unsupported => Refused::Unsupported,
                AdminError::Failed(why) => Refused::Failed(why),
            });
        }
        let mut c = if root {
            std::process::Command::new("sh")
        } else {
            eprintln!("doze needs sudo once: {}", plan.what);
            let mut s = std::process::Command::new("sudo");
            s.arg("sh");
            s
        };
        c.arg("-c").arg(&plan.script).stdout(io::stderr());
        return match c.status() {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(Refused::Failed(format!("the script exited with {s}"))),
            Err(e) => Err(Refused::Failed(e.to_string())),
        };
    }
    // PowerShell (Windows): UAC through doze-names, or the process's own shell when elevated already.
    let mut err = io::stderr();
    let mut o = Options {
        out: &mut err,
        how: How::Prompt,
    };
    doze_names::setup::run_plan(&mut o, plan).map_err(|e| {
        let why = match e {
            doze_names::Error::Privileged { why, .. } => why.to_string(),
            other => other.to_string(),
        };
        // ERROR_CANCELLED (1223): the person said no to UAC.
        if why.contains("1223") || why.to_ascii_lowercase().contains("cancel") {
            Refused::Cancelled
        } else {
            Refused::Failed(why)
        }
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
