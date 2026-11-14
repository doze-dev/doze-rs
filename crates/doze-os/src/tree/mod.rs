//! A process and everything it starts, as one unit that doze can stop gently, kill outright, and that cannot outlive
//! doze where the operating system allows it.
//!
//! - Dropping a [`Tree`] kills it: Windows closes its kill-on-close Job object, Unix kills its process group.
//! - If doze itself dies: on Windows the Job object takes the tree down (W0: under 3 ms); on Linux every tree leader
//!   gets SIGINT from `PR_SET_PDEATHSIG`; on macOS nothing can do this, and the daemon reconciles leftovers from its
//!   pidfiles when it starts.
//! - [`Tree::stop`] asks first ([`Stop`], declared per engine by its module) and kills the whole tree after `grace`.
//!
//! Linux caveat: the death signal follows the *thread* that spawned the tree. Spawn trees from a long-lived thread
//! (the runtime's main thread), never from `spawn_blocking`, whose threads exit when idle.

use std::ffi::OsString;
use std::io;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

/// How to ask a process tree to stop (PLAN.md D15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// SIGTERM on Unix; CTRL_BREAK on Windows.
    Terminate,
    /// SIGINT on Unix (postgres: fast shutdown); CTRL_BREAK on Windows.
    Interrupt,
    /// Run this command (program and arguments, no shell), then wait for the tree to exit.
    Command(Vec<OsString>),
}

/// How a stop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// The tree exited within the grace period.
    Graceful(ExitStatus),
    /// The grace period ran out and the tree was killed.
    Forced(ExitStatus),
}

/// A running process tree. See the module docs.
#[derive(Debug)]
pub struct Tree {
    child: tokio::process::Child,
    os: imp::Tree,
}

impl Tree {
    /// Starts `cmd` as the leader of a new tree. The caller sets arguments, environment and stdio.
    pub fn spawn(cmd: tokio::process::Command) -> io::Result<Tree> {
        let (child, os) = imp::spawn(cmd)?;
        Ok(Tree { child, os })
    }

    /// The leader's process id, while it has not been reaped.
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// The leader's stdout, if it was piped (once).
    pub fn take_stdout(&mut self) -> Option<tokio::process::ChildStdout> {
        self.child.stdout.take()
    }

    /// The leader's stderr, if it was piped (once).
    pub fn take_stderr(&mut self) -> Option<tokio::process::ChildStderr> {
        self.child.stderr.take()
    }

    /// Waits for the leader to exit.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait().await
    }

    /// The leader's exit status if it has exited.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Kills the whole tree now.
    pub fn kill(&mut self) -> io::Result<()> {
        self.os.kill()
    }

    /// Asks the tree to stop, waits up to `grace` for the leader to exit, and kills the tree if it does not. Either
    /// way, anything left in the tree afterwards is killed. [`Stop::Interrupt`] is Go's database sequence: SIGINT for up
    /// to 5 s (or `grace` if shorter), then SIGQUIT for 3 s, then the kill.
    pub async fn stop(&mut self, how: &Stop, grace: Duration) -> io::Result<Stopped> {
        let status = match how {
            Stop::Terminate => {
                let asked = self.os.signal(how);
                self.within(asked, grace).await
            }
            Stop::Interrupt => {
                let asked = self.os.signal(how);
                match self.within(asked, grace.min(INTERRUPT_PATIENCE)).await {
                    Some(st) => Some(st),
                    None => {
                        let asked = self.os.quit();
                        self.within(asked, QUIT_PATIENCE).await
                    }
                }
            }
            Stop::Command(argv) => {
                let asked = run_stop_command(argv, grace).await;
                self.within(asked, grace).await
            }
        };
        match status {
            Some(st) => {
                let st = st?;
                // Leftover members (a leader that exited before its children) go too.
                let _ = self.os.kill();
                Ok(Stopped::Graceful(st))
            }
            None => {
                self.os.kill()?;
                Ok(Stopped::Forced(self.child.wait().await?))
            }
        }
    }

    /// The leader's exit within `limit` once it was asked; `None` if it was not, or could not be asked (e.g. no
    /// console to send CTRL_BREAK through: straight to the kill).
    async fn within(
        &mut self,
        asked: io::Result<()>,
        limit: Duration,
    ) -> Option<io::Result<ExitStatus>> {
        asked.ok()?;
        tokio::time::timeout(limit, self.child.wait()).await.ok()
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.os.kill();
        }
    }
}

/// How long an interrupted process gets before SIGQUIT (Go's database stop: SIGINT 5 s, SIGQUIT 3 s, SIGKILL).
const INTERRUPT_PATIENCE: Duration = Duration::from_secs(5);
const QUIT_PATIENCE: Duration = Duration::from_secs(3);

/// Runs a module's stop command as a tree of its own (so nothing it starts outlives it, or doze), with null stdio (a
/// tool that leaves a server behind would otherwise hold captured pipes open forever: W0), bounded by `grace`.
async fn run_stop_command(argv: &[OsString], grace: Duration) -> io::Result<()> {
    let (prog, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty stop command"))?;
    let mut cmd = tokio::process::Command::new(prog);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut tree = Tree::spawn(cmd)?;
    let ended = tokio::time::timeout(grace, tree.wait()).await;
    // Whatever it left running goes; on a timeout the command itself goes too.
    let _ = tree.kill();
    match ended {
        Ok(st) => st.map(drop),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "stop command did not finish in time",
        )),
    }
}
