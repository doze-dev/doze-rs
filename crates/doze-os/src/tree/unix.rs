//! Unix: each tree is a process group led by its first process; on Linux the leader also gets SIGINT when doze dies.

use super::Stop;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions};
use std::io;

#[derive(Debug)]
pub(super) struct Tree {
    pgid: Option<Pid>,
}

pub(super) fn spawn(mut cmd: tokio::process::Command) -> io::Result<(tokio::process::Child, Tree)> {
    cmd.process_group(0);
    die_with_parent(&mut cmd);
    let child = spawn_from_a_lasting_thread(cmd)?;
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

#[cfg(not(target_os = "linux"))]
fn spawn_from_a_lasting_thread(
    mut cmd: tokio::process::Command,
) -> io::Result<tokio::process::Child> {
    cmd.spawn()
}

/// Linux's parent-death signal follows the *thread* that forked, not the process: a tree spawned from a thread that
/// later exits (a tokio blocking thread gone idle, a worker retired by `block_in_place`) would get SIGINT while doze
/// still runs (audit 2, minor 6). Every tree is therefore forked by one thread that lives as long as doze, which takes
/// the caller's runtime with it so the child is still driven by that runtime.
#[cfg(target_os = "linux")]
fn spawn_from_a_lasting_thread(cmd: tokio::process::Command) -> io::Result<tokio::process::Child> {
    use std::sync::{Mutex, OnceLock, mpsc};
    type Reply = mpsc::SyncSender<io::Result<tokio::process::Child>>;
    type Job = (tokio::process::Command, tokio::runtime::Handle, Reply);
    static SPAWNER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    let handle = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;
    let spawner = SPAWNER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        // If the thread cannot start, `rx` is dropped with it and every send below fails: trees are then spawned from
        // the calling thread, as before.
        let _ = std::thread::Builder::new()
            .name("doze-tree-spawner".into())
            .spawn(move || {
                for (mut cmd, handle, reply) in rx {
                    let _rt = handle.enter();
                    let _ = reply.send(cmd.spawn());
                }
            });
        Mutex::new(tx)
    });
    let (reply, answer) = mpsc::sync_channel(1);
    let job = (cmd, handle, reply);
    let sent = match spawner.lock() {
        Ok(tx) => tx.send(job).map_err(|back| back.0),
        Err(_) => Err(job),
    };
    match sent {
        Ok(()) => answer
            .recv()
            .map_err(|_| io::Error::other("the spawner thread is gone"))?,
        Err((mut cmd, ..)) => cmd.spawn(),
    }
}

impl Tree {
    pub(super) fn signal(&self, how: &Stop) -> io::Result<()> {
        let sig = match how {
            Stop::Interrupt => Signal::INT,
            _ => Signal::TERM,
        };
        self.send(sig)
    }

    /// The second stage of an interrupt: SIGQUIT (postgres: immediate shutdown).
    pub(super) fn quit(&self) -> io::Result<()> {
        self.send(Signal::QUIT)
    }

    pub(super) fn kill(&self) -> io::Result<()> {
        self.send(Signal::KILL)
    }

    /// Whether the leader has exited, leaving it unreaped (`WNOWAIT`): its zombie keeps the group id from being given
    /// to anyone else, so the group can still be signalled. An already reaped leader counts as exited.
    pub(super) fn exited(&self) -> io::Result<bool> {
        let Some(pid) = self.pgid else {
            return Ok(true);
        };
        let opts = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        loop {
            match rustix::process::waitid(WaitId::Pid(pid), opts) {
                Ok(st) => return Ok(st.is_some()),
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::CHILD) => return Ok(true),
                Err(e) => return Err(e.into()),
            }
        }
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
