//! Starting the doze daemon so it outlives the command that started it.
//!
//! Unix: a new session (`setsid`), so the terminal's hang-up and job-control signals do not reach it. Windows: no
//! window but **a console of its own** (`CREATE_NO_WINDOW`, not `DETACHED_PROCESS`): without a console the daemon
//! could not send CTRL_BREAK to its engines, and every graceful stop would become a kill. It also leaves the
//! starter's Job object when the job allows that, so closing the terminal that ran `doze up` does not end it.
//!
//! The caller sets stdio (null or a log file) on `cmd`; nothing is inherited from a terminal otherwise.

use std::io;
use std::process::Command;

/// Starts `cmd` detached and returns its pid. A thread of the starter's reaps it if it ends while the starter still
/// runs, so a long-lived starter (the app) keeps no zombie; a starter that exits first leaves it to init.
pub fn spawn_detached(cmd: Command) -> io::Result<u32> {
    let mut child = imp::spawn(cmd)?;
    let pid = child.id();
    let reaper = std::thread::Builder::new()
        .name("doze-reap".into())
        .spawn(move || child.wait().map(drop));
    // Without the thread the pid is still right; only the reaping is lost.
    drop(reaper);
    Ok(pid)
}

/// Starts `cmd` detached, as [`spawn_detached`], and hands back the child, for a starter that watches it fail at once
/// (the desktop app's `open`) before leaving it to run.
pub fn spawn_detached_child(cmd: Command) -> io::Result<std::process::Child> {
    imp::spawn(cmd)
}

#[cfg(unix)]
mod imp {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    #[allow(unsafe_code)] // the one pre_exec hook here; its SAFETY reason is below
    pub(super) fn spawn(mut cmd: Command) -> io::Result<std::process::Child> {
        let hook = || {
            rustix::process::setsid().map_err(io::Error::from)?;
            // The daemon is stopped with SIGTERM (`doze down`) and must hear it whoever started it. A starter run from
            // the desktop app inherits its thread's state (a libdispatch worker's), and the daemon then ignored
            // SIGTERM: `down` waited 20 s and gave up, and its engines outlived it. So the daemon starts with the
            // stop signals at their defaults and none blocked; it installs its own handlers.
            let mut none = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
            // SAFETY: sigemptyset only writes the set it is given, which lives on this stack; async-signal-safe.
            unsafe { libc::sigemptyset(none.as_mut_ptr()) };
            // SAFETY: sigemptyset above initialised the set.
            let none = unsafe { none.assume_init() };
            // SAFETY: sigprocmask reads the set and writes no old one; async-signal-safe, no locks, no allocation.
            unsafe { libc::sigprocmask(libc::SIG_SETMASK, &raw const none, std::ptr::null_mut()) };
            for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
                // SAFETY: signal with SIG_DFL installs no handler and is async-signal-safe.
                unsafe { libc::signal(sig, libc::SIG_DFL) };
            }
            Ok(())
        };
        // SAFETY: the hook runs in the child between fork and exec and only makes the setsid syscall through rustix
        // and the signal calls above: no allocation, no locks, async-signal-safe.
        unsafe { cmd.pre_exec(hook) };
        cmd.spawn()
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use windows_sys::Win32::System::Threading::{
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
    };

    pub(super) fn spawn(mut cmd: Command) -> io::Result<std::process::Child> {
        let base = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP;
        cmd.creation_flags(base | CREATE_BREAKAWAY_FROM_JOB);
        match cmd.spawn() {
            Ok(child) => Ok(child),
            // The starter's job forbids breaking away: start inside it (it then ends with that job).
            Err(e) if e.raw_os_error() == Some(5) => {
                cmd.creation_flags(base);
                cmd.spawn()
            }
            Err(e) => Err(e),
        }
    }
}
