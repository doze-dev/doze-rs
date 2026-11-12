//! Unix: each tree is a process group led by its first process; on Linux the leader also gets SIGINT when doze dies.

use super::Stop;
use rustix::process::{Pid, Signal};
use std::io;

#[derive(Debug)]
pub(super) struct Tree {
    pgid: Option<Pid>,
}

pub(super) fn spawn(mut cmd: tokio::process::Command) -> io::Result<(tokio::process::Child, Tree)> {
    cmd.process_group(0);
    die_with_parent(&mut cmd);
    let child = cmd.spawn()?;
    let pgid = child
        .id()
        .and_then(|p| i32::try_from(p).ok())
        .and_then(Pid::from_raw);
    Ok((child, Tree { pgid }))
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)] // the one pre_exec hook; its SAFETY reason is below
fn die_with_parent(cmd: &mut tokio::process::Command) {
    let parent = rustix::process::getpid();
    let hook = move || {
        rustix::process::set_parent_process_death_signal(Some(Signal::INT))?;
        // The parent may have died between fork and prctl; then nobody will send the signal.
        if rustix::process::getppid() != Some(parent) {
            return Err(io::Error::other("doze exited while starting this process"));
        }
        Ok(())
    };
    // SAFETY: the hook runs in the child between fork and exec; it only makes raw syscalls (prctl, getppid) through
    // rustix, allocates nothing and takes no locks, so it is async-signal-safe.
    unsafe { cmd.pre_exec(hook) };
}

#[cfg(not(target_os = "linux"))]
fn die_with_parent(_cmd: &mut tokio::process::Command) {}

impl Tree {
    pub(super) fn signal(&self, how: &Stop) -> io::Result<()> {
        let sig = match how {
            Stop::Interrupt => Signal::INT,
            _ => Signal::TERM,
        };
        self.send(sig)
    }

    pub(super) fn kill(&self) -> io::Result<()> {
        self.send(Signal::KILL)
    }

    fn send(&self, sig: Signal) -> io::Result<()> {
        let pgid = self
            .pgid
            .ok_or_else(|| io::Error::other("no process group"))?;
        match rustix::process::kill_process_group(pgid, sig) {
            // Already gone is not an error for a stop.
            Err(rustix::io::Errno::SRCH) => Ok(()),
            r => r.map_err(io::Error::from),
        }
    }
}
