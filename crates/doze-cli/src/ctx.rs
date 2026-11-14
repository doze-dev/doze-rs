//! Everything a run touches outside itself, passed in: the environment, the two output streams and whether each is a
//! terminal, the doze home, the registry, the clock, and the two things it may start (a browser, a background check).
//! `main` fills it from the process; the tests fill it with buffers, a temporary home and a registry on disk.

use crate::registry;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::SystemTime;

/// What a run may ask of the machine it runs on.
pub trait Host {
    /// An environment variable (unset and empty are the same).
    fn var(&self, name: &str) -> Option<String>;
    /// Opens a web page in the person's browser.
    fn open_url(&self, url: &str) -> io::Result<()>;
    /// Starts `doze <args>` detached, never waited for (the background refresh).
    fn spawn_background(&self, args: &[&str]) -> io::Result<()>;
    /// This program's own path, which says how doze was installed.
    fn exe(&self) -> Option<PathBuf>;
}

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
