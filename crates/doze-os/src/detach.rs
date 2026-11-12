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

/// Starts `cmd` detached and returns its pid. Nothing waits for it.
pub fn spawn_detached(cmd: Command) -> io::Result<u32> {
    imp::spawn(cmd)
}

#[cfg(unix)]
mod imp {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    #[allow(unsafe_code)] // the one pre_exec hook here; its SAFETY reason is below
    pub(super) fn spawn(mut cmd: Command) -> io::Result<u32> {
        let hook = || rustix::process::setsid().map(drop).map_err(io::Error::from);
        // SAFETY: the hook runs in the child between fork and exec and only makes the setsid syscall through rustix:
        // no allocation, no locks, async-signal-safe.
        unsafe { cmd.pre_exec(hook) };
        let child = cmd.spawn()?;
        Ok(child.id())
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

    pub(super) fn spawn(mut cmd: Command) -> io::Result<u32> {
        let base = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP;
        cmd.creation_flags(base | CREATE_BREAKAWAY_FROM_JOB);
        match cmd.spawn() {
            Ok(child) => Ok(child.id()),
            // The starter's job forbids breaking away: start inside it (it then ends with that job).
            Err(e) if e.raw_os_error() == Some(5) => {
                cmd.creation_flags(base);
                Ok(cmd.spawn()?.id())
            }
            Err(e) => Err(e),
        }
    }
}
