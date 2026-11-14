//! Everything a run touches outside itself, passed in: the environment, the two output streams and whether each is a
//! terminal, the doze home, the registry, the clock, and the two things it may start (a browser, a background check).
//! `main` fills it from the process; the tests fill it with buffers, a temporary home and a registry on disk.

use crate::registry;
use doze_os::interrupt::Interrupt;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::SystemTime;

/// What a run may ask of the machine it runs on.
pub trait Host {
    /// An environment variable (unset and empty are the same).
    fn var(&self, name: &str) -> Option<String>;
    /// The environment as doze.hcl's `env()` reads it: the process's own, or a test's.
    fn env_lookup(&self) -> doze_hcl::funcs::EnvLookup {
        std::sync::Arc::new(|name: &str| {
            if name.is_empty() || name.contains(['=', '\0']) {
                return None;
            }
            std::env::var(name).ok()
        })
    }
    /// Opens a web page in the person's browser.
    fn open_url(&self, url: &str) -> io::Result<()>;
    /// Starts `doze <args>` detached, never waited for (the background refresh).
    fn spawn_background(&self, args: &[&str]) -> io::Result<()>;
    /// This program's own path, which says how doze was installed.
    fn exe(&self) -> Option<PathBuf>;
    /// The time now, for what a follow redraws ("sleeps in 4m"). A test's clock stands still.
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
    /// Takes Ctrl-C and SIGTERM over for a command that follows the daemon until stopped, so it ends cleanly
    /// (its last line written, exit 0). Only such a command asks; for the rest Ctrl-C ends the process as usual.
    fn watch_interrupt(&self) -> Interrupt {
        Interrupt::never()
    }
    /// Asks the person at the terminal a yes-or-no question (the question is already on stderr); `None` when there
    /// is no one to ask.
    fn ask(&self) -> Option<bool> {
        None
    }
    /// Reads one line the person types at the terminal (the question is already written), without its newline;
    /// `None` when there is no one to ask or the input ended (`init --wizard`).
    fn read_line(&self) -> Option<String> {
        None
    }
    /// Runs `program` with `args`, this terminal its terminal, and gives its exit code (`doze open` hands the
    /// command to the app). Nothing is run by default: only the real process and a test's recorder do anything.
    fn run(&self, program: &std::path::Path, args: &[String]) -> io::Result<u8> {
        let _ = (program, args);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "nothing is run here",
        ))
    }
    /// The folders an installed Doze.app is looked for in: on a Mac `~/Applications` and `/Applications`.
    fn applications(&self) -> Vec<PathBuf> {
        if !crate::platform::os_arch().starts_with("darwin") {
            return Vec::new();
        }
        let mut out: Vec<PathBuf> = std::env::home_dir()
            .map(|h| h.join("Applications"))
            .into_iter()
            .collect();
        out.push(PathBuf::from("/Applications"));
        out
    }
    /// Where the machine's system files are (`/etc/resolver`, the launchd job, …): the real ones, or a test's
    /// temporary root, so nothing a test does reads or writes the machine's.
    fn setup_paths(&self) -> doze_names::setup::Paths {
        doze_names::setup::Paths::system()
    }
    /// Runs the one privileged script of a setup, trust or repair as root: under sudo on this terminal, or, with
    /// `prompt`, behind the system's administrator prompt (the app's way). Nothing is run by default: only the real
    /// process and a test's recorder do anything.
    fn privileged(&self, plan: &doze_names::setup::Plan, prompt: bool) -> Result<(), Refused> {
        let _ = (plan, prompt);
        Err(Refused::Unsupported)
    }
    /// How long a repair waits for the network daemon launchd starts a beat after the script returns.
    fn settle(&self) -> std::time::Duration {
        std::time::Duration::from_secs(6)
    }
}

/// Why a privileged script did not run, or did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The person closed the prompt (or said no to sudo).
    Cancelled,
    /// There is no way to ask for root here.
    Unsupported,
    /// It ran and failed; what it said.
    Failed(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // The app tells a cancel from a failure by these words in the command's output (Go
            // `adminprompt.CancelledText`).
            Refused::Cancelled => f.write_str(CANCELLED),
            Refused::Unsupported => f.write_str(
                "no way to ask for an administrator's password here: run the command in a terminal",
            ),
            Refused::Failed(why) => f.write_str(why),
        }
    }
}

/// What a cancelled prompt says (Go `adminprompt.CancelledText`; the app reads it).
pub const CANCELLED: &str = "the administrator prompt was cancelled";

/// The variable the app sets on the doze commands it runs, so their privileged step asks with the system's prompt
/// instead of sudo (Go `adminprompt.Env`).
pub const PROMPT_ENV: &str = "DOZE_ADMIN_PROMPT";

/// Which standard streams are terminals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Terminals {
    pub stdin: bool,
    pub stdout: bool,
    pub stderr: bool,
}

/// One run's surroundings.
pub struct Ctx<'a> {
    pub host: &'a dyn Host,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub term: Terminals,
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub registry: registry::Config,
    pub now: SystemTime,
}

impl std::fmt::Debug for Ctx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx")
            .field("term", &self.term)
            .field("home", &self.home)
            .field("cwd", &self.cwd)
            .finish_non_exhaustive()
    }
}

impl Ctx<'_> {
    /// A variable that is set to something.
    pub fn var(&self, name: &str) -> Option<String> {
        self.host.var(name).filter(|v| !v.is_empty())
    }

    /// Whether a person is at a terminal: input and both outputs are one.
    pub fn interactive(&self) -> bool {
        self.term.stdin && self.term.stdout && self.term.stderr
    }

    /// Seconds since the epoch, for the state files.
    pub fn unix_now(&self) -> u64 {
        self.now
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// The version of this doze: set by the release build (`DOZE_VERSION`), `dev` otherwise.
pub const VERSION: &str = match option_env!("DOZE_VERSION") {
    Some(v) => v,
    None => "dev",
};
