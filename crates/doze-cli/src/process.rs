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

    fn ask_or(&self, interrupt: &Interrupt) -> Option<bool> {
        answer(
            || {
                let mut line = String::new();
                io::stdin().read_line(&mut line).map(|_| line)
            },
            interrupt,
        )
    }

    fn run(&self, program: &std::path::Path, args: &[String]) -> io::Result<u8> {
        self.run_env(program, args, &[])
    }

    fn run_env(
        &self,
        program: &std::path::Path,
        args: &[String],
        env: &[(String, String)],
    ) -> io::Result<u8> {
        // The command runs in the foreground as a shell's job (`doze_os::foreground`): Ctrl-C at the terminal is the
        // command's alone (a shell cancels its query and stays), and a SIGINT or SIGTERM sent to doze by another
        // process (an IDE's stop) is passed on to it, once. doze waits for it and gives its code. Go's `doze run` died
        // on Ctrl-C, leaving a command that stays without doze. One ended by a signal gives 128 + it (130).
        let mut cmd = Command::new(program);
        cmd.args(args).envs(env.iter().map(|(k, v)| (k, v)));
        let status = doze_os::foreground::run(cmd)?;
        Ok(doze_os::process::exit_code(&status))
    }

    fn read_line(&self) -> Option<String> {
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_owned()),
        }
    }

    fn privileged(&self, plan: &Plan, prompt: bool) -> Result<(), Refused> {
        crate::machine::run_as_root(plan, prompt)
    }
}

/// A yes-or-no answer read by `read` (on a thread of its own: a read of the terminal cannot be broken off), or "no" as
/// soon as `interrupt` is raised; `None` when the input cannot be read.
fn answer(
    read: impl FnOnce() -> io::Result<String> + Send + 'static,
    interrupt: &Interrupt,
) -> Option<bool> {
    use std::sync::mpsc::{RecvTimeoutError, channel};
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("doze-ask".into())
        .spawn(move || {
            let _ = tx.send(read());
        })
        .ok()?;
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(Ok(line)) => {
                return Some(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"));
            }
            Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => return None,
            Err(RecvTimeoutError::Timeout) if interrupt.raised() => {
                // The question's line is left open by the person's ^C: end it.
                let _ = writeln!(io::stderr());
                return Some(false);
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Runs `argv` (`argv[0]` is the program) as this process and gives the exit code.
pub fn main(argv: Vec<OsString>) -> u8 {
    // The network daemon dns-setup installs is this program: it runs before anything else (Go `names.Helper()`).
    if argv.get(1).is_some_and(|a| a == doze_names::HELPER_ARG) {
        return crate::machine::netd(argv.get(2..).unwrap_or_default());
    }
    // The stack's daemon (`ctl::start` runs `<this program> __daemon --config <path>` detached): whatever program
    // carries the CLI is its own daemon, so the app needs no `doze` beside it.
    if argv.get(1).is_some_and(|a| a == doze_daemon::ctl::ARG) {
        let args: Vec<String> = argv
            .iter()
            .skip(2)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        return match doze_daemon::entry::main(&args, crate::ctx::VERSION) {
            std::process::ExitCode::SUCCESS => 0,
            _ => 1,
        };
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
    let key = test_registry_key().unwrap_or(key);
    let registry = registry::Config {
        bases: registry::bases_from_env(),
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Ctrl-C while the person has not answered is "no", at once, though the read of the terminal never returns.
    #[test]
    fn ctrl_c_at_a_question_is_no() {
        let interrupt = Interrupt::never();
        let raise = interrupt.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            raise.raise();
        });
        let never_answers = || {
            std::thread::sleep(std::time::Duration::from_secs(3600));
            Ok(String::new())
        };
        let started = std::time::Instant::now();
        assert_eq!(answer(never_answers, &interrupt), Some(false));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn an_answer_is_read_as_before() {
        let i = Interrupt::never();
        assert_eq!(answer(|| Ok("y\n".into()), &i), Some(true));
        assert_eq!(answer(|| Ok(" Yes \n".into()), &i), Some(true));
        assert_eq!(answer(|| Ok("\n".into()), &i), Some(false));
        assert_eq!(answer(|| Err(io::Error::other("closed")), &i), None);
    }
}
