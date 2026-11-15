//! Machine setup: the one-time, privileged work that makes `.doze` resolve (feature `setup`).
//!
//! Any doze program can run it and running it twice changes nothing, so `doze dns-setup`, `doze-aws dns-setup` and
//! `doze-kafka dns-setup` are the same command: whichever one is installed can set the machine up.
//!
//! Everything privileged happens in one script under one prompt, and every part is reported on its own by
//! [`Setup::check`], which needs no privilege, so a half-finished install says which half. The parts are data: each
//! system's module turns what it found on the machine into the steps of the check and the lines of the script, from
//! one list, so the prompt's words, the script and the report cannot drift apart. The script builders are plain
//! functions of what was found, so every system's script is tested on every system.
//!
//! - macOS (`mac.rs`): the network daemon (launchd) and `/etc/resolver/doze`.
//! - Linux (`linux.rs`): the `doze0` interface with 192.0.2.53, the systemd-resolved or dnsmasq route, the daemon's
//!   systemd unit, the marked `/etc/hosts` block and the privileged-port sysctl.
//! - Windows (`windows.rs`): a Name Resolution Policy Table rule sending `.doze` to the resolver.

pub mod hosts;
mod linux;
mod linux_units;
mod mac;
mod run;
pub mod trust;
mod trust_scripts;
mod who;
mod windows;

#[doc(hidden)]
pub use linux_units::netd_serves;
pub use run::{How, Options, pasteable, shell_quote};
pub use who::{setup_home, setup_user};

use crate::Error;
use std::fmt;
use std::path::{Path, PathBuf};

/// One part of the setup, reported on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub done: bool,
    pub detail: String,
}

impl Step {
    pub(crate) fn new(name: &str, done: bool, detail: impl Into<String>) -> Step {
        Step {
            name: name.to_string(),
            done,
            detail: detail.into(),
        }
    }
}

/// What [`Setup::check`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub platform: String,
    pub steps: Vec<Step>,
}

impl Status {
    /// Whether every step is in place (and there is at least one).
    pub fn ok(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.done)
    }
}

impl fmt::Display for Status {
    /// One line per step.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for st in &self.steps {
            let mark = if st.done { "✓" } else { "✗" };
            writeln!(f, "{mark} {:<22} {}", st.name, st.detail)?;
        }
        Ok(())
    }
}

/// The shell a privileged script is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// `sh`, on macOS and Linux.
    Sh,
    /// Windows PowerShell 5.1, which every Windows since 10 has.
    PowerShell,
}

/// The one privileged script of a run, and what it is for in a few words (the prompt says them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub what: String,
    pub shell: Shell,
    pub script: String,
}

/// The system, decided at run time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Windows,
    Other,
}

impl Os {
    /// This machine's.
    pub fn native() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Other
        }
    }
}

/// Every system file setup reads or writes, so a test (or a packager's staging root) can move them all and nothing in
/// a test run ever touches the real ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub hosts: PathBuf,
    pub resolv_conf: PathBuf,
    /// macOS: where a domain's resolver file goes (`/etc/resolver`).
    pub resolver_dir: PathBuf,
    /// macOS: the daemon's launchd job, the copy it runs, its log, and the job from before the daemon.
    pub launchd_job: PathBuf,
    pub helper: PathBuf,
    pub helper_log: PathBuf,
    pub old_launchd_job: PathBuf,
    /// Linux: the sysctl drop-in, dnsmasq's drop-in folder, the zone's interface unit, the daemon's unit and copy.
    pub sysctl: PathBuf,
    pub dnsmasq_dir: PathBuf,
    pub zone_unit: PathBuf,
    pub netd_unit: PathBuf,
    pub netd_program: PathBuf,
    /// Linux: what the kernel lets an unprivileged process bind from.
    pub port_start: PathBuf,
}

impl Paths {
    /// The real ones.
    pub fn system() -> Paths {
        Paths::under(Path::new("/"))
    }

    /// Every path re-rooted under `root`.
    pub fn under(root: &Path) -> Paths {
        let p = |s: &str| root.join(s);
        Paths {
            hosts: p("etc/hosts"),
            resolv_conf: p("etc/resolv.conf"),
            resolver_dir: p("etc/resolver"),
            launchd_job: p("Library/LaunchDaemons/dev.doze.netd.plist"),
            helper: p("Library/PrivilegedHelperTools/dev.doze.netd"),
            helper_log: p("var/log/doze-netd.log"),
            old_launchd_job: p("Library/LaunchDaemons/dev.doze.loopback.plist"),
            sysctl: p("etc/sysctl.d/60-doze.conf"),
            dnsmasq_dir: p("etc/dnsmasq.d"),
            zone_unit: p("etc/systemd/system/doze-names.service"),
            netd_unit: p("etc/systemd/system/doze-netd.service"),
            netd_program: p("usr/local/libexec/doze-netd"),
            port_start: p("proc/sys/net/ipv4/ip_unprivileged_port_start"),
        }
    }
}

/// The program setup installs as the network daemon: a copy of it, started with `args` and then the registry (and on
/// macOS the uid and gid to drop to).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Daemon {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl Daemon {
    /// This program, started with [`crate::HELPER_ARG`]: it must turn into the daemon when it sees that argument, as
    /// the `doze-names` binary does. A copy, so it does not vanish when the program is upgraded or moved.
    pub fn this_program() -> Result<Daemon, Error> {
        let program = std::env::current_exe().map_err(|e| {
            Error::Setup(format!(
                "cannot find this program to install as the daemon: {e}"
            ))
        })?;
        Ok(Daemon {
            program,
            args: vec![crate::HELPER_ARG.to_string()],
        })
    }
}

/// A machine to set up: which system, where its files are, whose zone the daemon serves, and what it runs.
#[derive(Debug, Clone)]
pub struct Setup {
    pub os: Os,
    pub paths: Paths,
    /// The names folder of the person running setup (also under sudo: [`setup_home`]).
    pub home: PathBuf,
    pub daemon: Daemon,
    /// The person running setup (also under sudo: [`setup_user`]); `None` where there are no numeric ids, or when
    /// run as root with no `SUDO_UID`.
    pub user: Option<(u32, u32)>,
}

impl Setup {
    /// This machine, its real files, and the person running the program.
    pub fn native(daemon: Daemon) -> Setup {
        Setup {
            os: Os::native(),
            paths: Paths::system(),
            home: setup_home(),
            daemon,
            user: setup_user().ok(),
        }
    }

    /// The registry the daemon serves.
    pub(crate) fn registry(&self) -> PathBuf {
        self.home.join(crate::registry::FILE_NAME)
    }

    /// The person setup is for, or why it cannot know.
    pub(crate) fn person(&self) -> Result<(u32, u32), Error> {
        self.user.ok_or_else(|| {
            Error::Setup(
                "setup has to know whose services to serve: run it as yourself, not as root".into(),
            )
        })
    }

    /// What is in place, without any privilege.
    pub fn check(&self) -> Status {
        match self.os {
            Os::Mac => mac::check(self),
            Os::Linux => linux::check(self, &linux::Facts::read(self)),
            Os::Windows => windows::check(&windows::Facts::read()),
            Os::Other => unsupported(),
        }
    }

    /// The script that sets the machine up; `None` when everything is in place already.
    pub fn install_plan(&self) -> Result<Option<Plan>, Error> {
        match self.os {
            Os::Mac => mac::install_plan(self),
            Os::Linux => linux::install_plan(self, &linux::Facts::read(self)).map(Some),
            Os::Windows => windows::install_plan(&windows::Facts::read()),
            Os::Other => Err(Error::Setup(NOT_HERE.into())),
        }
    }

    /// The script that removes everything an install wrote, and the domains' routes and the authority's trust.
    pub fn uninstall_plan(&self) -> Result<Plan, Error> {
        let cleanup = trust::cleanup_script(self.os, &self.home);
        match self.os {
            Os::Mac => Ok(mac::uninstall_plan(self, &cleanup)),
            Os::Linux => linux::uninstall_plan(self, &cleanup),
            Os::Windows => Ok(windows::uninstall_plan(&cleanup)),
            Os::Other => Err(Error::Setup(NOT_HERE.into())),
        }
    }

    /// Sets the machine up, in one prompt; idempotent.
    pub fn install(&self, o: &mut Options<'_>) -> Result<(), Error> {
        let Some(plan) = self.install_plan()? else {
            writeln!(o.out, "✓ already set up — nothing to do")?;
            return Ok(());
        };
        run::run(o, &plan)?;
        if matches!(o.how, How::Print) {
            return Ok(());
        }
        match self.os {
            Os::Mac => mac::after_install(self, o),
            _ => {
                write!(o.out, "{}", self.check())?;
                if self.os == Os::Linux {
                    linux::after_install(self, o)?;
                }
                Ok(())
            }
        }
    }

    /// Removes everything an install wrote, in one prompt, then what the person owns: the authority's files and the
    /// routes record.
    pub fn uninstall(&self, o: &mut Options<'_>) -> Result<(), Error> {
        let plan = self.uninstall_plan()?;
        run::run(o, &plan)?;
        if !matches!(o.how, How::Print) {
            trust::cleanup_after(&self.home);
            writeln!(o.out, "✓ removed")?;
        }
        Ok(())
    }
}

const NOT_HERE: &str = ".doze names are not supported on this system: nothing was changed";

fn unsupported() -> Status {
    Status {
        platform: std::env::consts::OS.to_string(),
        steps: vec![Step::new("resolver", false, "not supported on this system")],
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
