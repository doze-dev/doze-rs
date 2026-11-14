//! A run on the real process: it fills [`crate::run`]'s surroundings from this process (the environment, the terminals,
//! the doze home, the official registry) and gives the exit code. Both programs that carry the CLI call it: the `doze`
//! binary (`src/main.rs`) and the desktop app, where `doze-app <command>` is the CLI (owner decision, 2026-11-14).

use crate::ctx::Refused;
use crate::diag::{Diagnostic, ids};
use crate::registry::{self, signed};
use crate::{Ctx, Host, Terminals};
use doze_names::setup::Plan;
use doze_os::interrupt::Interrupt;
use std::ffi::OsString;
use std::io::{self, BufWriter, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::SystemTime;

/// The real machine.
#[derive(Debug)]
struct Process;

impl Host for Process {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn open_url(&self, url: &str) -> io::Result<()> {
        doze_os::browse::open(url)
    }

    fn spawn_background(&self, args: &[&str]) -> io::Result<()> {
        let mut cmd = Command::new(std::env::current_exe()?);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        doze_os::detach::spawn_detached(cmd).map(drop)
    }

    fn exe(&self) -> Option<PathBuf> {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.canonicalize().ok())
    }

    fn watch_interrupt(&self) -> Interrupt {
        Interrupt::watch().unwrap_or_else(|_| Interrupt::never())
    }

    fn ask(&self) -> Option<bool> {
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).ok()?;
        Some(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
    }

    fn privileged(&self, plan: &Plan, prompt: bool) -> Result<(), Refused> {
        crate::machine::run_as_root(plan, prompt)
    }
}

/// Runs `argv` (`argv[0]` is the program) as this process and gives the exit code.
pub fn main(argv: Vec<OsString>) -> u8 {
    // The network daemon dns-setup installs is this program: it runs before anything else (Go `names.Helper()`).
    if argv.get(1).is_some_and(|a| a == doze_names::HELPER_ARG) {
        return crate::machine::netd(argv.get(2..).unwrap_or_default());
    }
    // One download for the daemon, in a process of its own (core plan F1): stdin to stdout, nothing else.
    if argv.get(1).is_some_and(|a| a == crate::fetch_child::ARG) {
        return match surroundings() {
            Ok((home, _, registry)) => crate::fetch_child::serve(&home, registry),
            Err(_) => 1,
        };
    }
    let term = Terminals {
        stdin: io::stdin().is_terminal(),
        stdout: io::stdout().is_terminal(),
        stderr: io::stderr().is_terminal(),
    };
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut err = io::stderr();
    let exit = match surroundings() {
        Ok((home, cwd, registry)) => {
            let mut ctx = Ctx {
                host: &Process,
                out: &mut out,
                err: &mut err,
                term,
                home,
                cwd,
                registry,
                now: SystemTime::now(),
            };
            crate::run(argv, &mut ctx)
        }
        Err(d) => {
            let _ = write!(
                err,
                "error[{}]: {}\n  fix: {}\n",
                d.id.code,
                d.message,
                d.fix.unwrap_or_default()
            );
            d.id.exit
        }
    };
    let _ = out.flush();
    exit.code()
}

/// The doze home, the working directory and the official registry, or why there are none.
fn surroundings() -> Result<(PathBuf, PathBuf, registry::Config), Diagnostic> {
    let home = doze_os::home::doze_home().map_err(|e| {
        Diagnostic::new(
            ids::HOME_UNUSABLE,
            format!("doze has no home directory ({e})"),
        )
        .fix("set DOZE_HOME to a directory doze can write")
    })?;
    let cwd = working_dir().map_err(|e| {
        Diagnostic::new(
            ids::HOME_UNUSABLE,
            format!("the current directory can't be read ({e})"),
        )
        .fix("cd to a directory that exists")
    })?;
    let key = signed::builtin_key().ok_or_else(|| {
        Diagnostic::new(
            ids::BUG,
            "the registry key built into this doze is not a key",
        )
        .fix("doze bug")
    })?;
    let registry = registry::Config {
        bases: registry::bases_from_env(),
        key,
        budget: registry::BUDGET,
    };
    Ok((home, cwd, registry))
}

/// The working directory as the shell names it: `$PWD` when it is this directory (through a symlink, `/tmp` on a
/// Mac), as Go's `os.Getwd` gives it. A project's state folder is named from its path, so the Go doze and this one
/// must spell the path the same way to find the same daemon.
fn working_dir() -> io::Result<PathBuf> {
    let real = std::env::current_dir()?;
    let shell = std::env::var_os("PWD").map(PathBuf::from);
    Ok(match shell {
        Some(p) if p.is_absolute() && p.canonicalize().ok() == real.canonicalize().ok() => p,
        _ => real,
    })
}
