//! `doze`: the process around [`doze_cli::run`]. It fills the run's surroundings from this process (the environment,
//! the terminals, the doze home, the official registry) and exits with the run's code.

use doze_cli::diag::{Diagnostic, ids};
use doze_cli::registry::{self, Base, signed};
use doze_cli::{Ctx, Exit, Host, Terminals};
use std::io::{self, BufWriter, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
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
}

fn main() -> ExitCode {
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
            doze_cli::run(std::env::args_os().collect(), &mut ctx)
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
    ExitCode::from(exit.code())
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
    let cwd = std::env::current_dir().map_err(|e| {
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
    let key = test_registry_key().unwrap_or(key);
    let registry = registry::Config {
        bases: registry::HOSTS
            .iter()
            .map(|h| Base::Http((*h).to_string()))
            .collect(),
        key,
        budget: registry::BUDGET,
    };
    Ok((home, cwd, registry))
}

/// The key a debug build trusts instead of the built-in one when `DOZE_TEST_REGISTRY_KEY` names one: tests that run
/// the real binary sign their sandbox registry with the throwaway fixture key, because nothing they could check in is
/// signed with the production key. A release build, the only kind anyone is given, has no such variable: this is
/// compiled out, and `release_ignores_the_test_key` in tests/terminal.rs holds it to that.
#[cfg(debug_assertions)]
fn test_registry_key() -> Option<ed25519_dalek::VerifyingKey> {
    signed::parse_key(&std::env::var("DOZE_TEST_REGISTRY_KEY").ok()?)
}

#[cfg(not(debug_assertions))]
fn test_registry_key() -> Option<ed25519_dalek::VerifyingKey> {
    None
}

// The exit class of a run is all `main` reports; keep the type in the signature honest.
const _: fn(Exit) -> u8 = Exit::code;
