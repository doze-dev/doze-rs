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

    /// Waits for the leader to exit, kills whatever it left in its tree, and returns how it ended. The leftovers are
    /// signalled before the leader is reaped: its unreaped exit still holds the group id (Unix), so the signal cannot
    /// reach a process that took the id over.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.exited().await?;
        let _ = self.os.kill();
        self.child.wait().await
    }

    /// The leader's exit status if it has exited (whatever it left in its tree is killed first, as in [`Tree::wait`]).
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.running()? {
            return Ok(None);
        }
        let _ = self.os.kill();
        self.child.try_wait()
    }

    /// Whether the leader is still running. It is not reaped when it has exited.
    pub fn running(&mut self) -> io::Result<bool> {
        #[cfg(unix)]
        return self.os.exited().map(|e| !e);
        #[cfg(windows)]
        return self.child.try_wait().map(|s| s.is_none());
    }

    /// Waits until the leader has exited, without reaping it.
    async fn exited(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        return self.child.wait().await.map(drop);
        #[cfg(unix)]
        {
            // No readiness event for an exit that must not be reaped: poll, quickly at first.
            let mut pause = Duration::from_millis(2);
            while self.running()? {
                tokio::time::sleep(pause).await;
                pause = (pause * 2).min(Duration::from_millis(50));
            }
            Ok(())
        }
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
        self.ended(status).await
    }

    /// Stops a tree its caller has already asked to stop by its own means (a stop command the caller ran and recorded
    /// itself): waits up to `grace` for the leader to exit, then kills the tree; anything left in it goes either way.
    /// An `Err` in `asked` (the asking failed) goes straight to the kill.
    pub async fn stop_asked(
        &mut self,
        asked: io::Result<()>,
        grace: Duration,
    ) -> io::Result<Stopped> {
        let status = self.within(asked, grace).await;
        self.ended(status).await
    }

    async fn ended(&mut self, status: Option<io::Result<ExitStatus>>) -> io::Result<Stopped> {
        match status {
            // Leftover members (a leader that exited before its children) went in `wait`.
            Some(st) => Ok(Stopped::Graceful(st?)),
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
        match tokio::time::timeout(limit, self.exited()).await {
            Err(_) => None,
            Ok(Err(e)) => Some(Err(e)),
            Ok(Ok(())) => Some(self.wait().await),
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        // Not reaped yet: the leader (running, or its unreaped exit) still holds the group id, and what it left behind
        // goes with it.
        if self.child.id().is_some() {
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
    // Whatever it left running goes with it (`wait`); on a timeout the command itself goes too (drop).
    match tokio::time::timeout(grace, tree.wait()).await {
        Ok(st) => st.map(drop),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "stop command did not finish in time",
        )),
    }
}
