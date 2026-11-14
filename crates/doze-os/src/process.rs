//! Processes by pid: whether one is still there (how the shared `.doze` registry decides a name's holder is gone: a
//! crashed process runs no shutdown hook, so liveness is by pid, not by a clean release), killing a tree a previous
//! daemon left behind, and saying how a process ended in the words doze has always used.

use std::io;
use std::process::ExitStatus;

/// Whether a process with this pid exists. On Unix a process owned by someone else counts (the signal-0 probe says
/// `EPERM`: it exists). On Windows a process that has exited does not count even while some handle keeps its pid
/// reserved (Go's `os.FindProcess` said it did).
pub fn alive(pid: u32) -> bool {
    pid != 0 && imp::alive(pid)
}

/// Whether anything recorded under this pid is left: the process itself, or (Unix) a member of the process group it
/// led. On Linux a dying daemon's PDEATHSIG reaches only a tree's leader, so its children can outlive it in the group.
pub fn tree_alive(pid: u32) -> bool {
    pid != 0 && (imp::alive(pid) || imp::group_alive(pid))
}

/// Whether recorded pids must be swept at start: not on Windows, where a dead daemon's Job objects already ended every
/// tree it started, so a recorded pid that is alive belongs to someone else now.
pub fn sweep_needed() -> bool {
    cfg!(not(windows))
}

/// Kills, by its exact pid, a process tree a previous daemon recorded and could not stop (it was killed itself). On
/// Unix the pid led its own process group ([`crate::tree::Tree`]), so the whole group goes; if the group is gone the
/// process alone is tried. On Windows the process is terminated (its Job object died with the old daemon, which
/// already ended the rest of the tree). A pid that is not there is not an error.
pub fn kill_recorded(pid: u32) -> io::Result<()> {
    if pid == 0 {
        return Ok(());
    }
    imp::kill_recorded(pid)
}

/// How a process ended, in Go's words (the journal and `status` show them): "exited with status 1", or for a
/// signal "exited: signal: killed".
pub fn exit_text(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exited with status {code}"),
        None => format!("exited: {}", imp::signal_text(status)),
    }
}

#[cfg(unix)]
mod imp {
    use rustix::io::Errno;
    use rustix::process::{Pid, Signal, kill_process, kill_process_group, test_kill_process};
    use std::os::unix::process::ExitStatusExt;

    pub(super) fn alive(pid: u32) -> bool {
        let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
            return false;
        };
        match test_kill_process(pid) {
            Ok(()) => true,
            Err(e) => e == Errno::PERM,
        }
    }

    pub(super) fn group_alive(pid: u32) -> bool {
        let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
            return false;
        };
        match rustix::process::test_kill_process_group(pid) {
            Ok(()) => true,
            Err(e) => e == Errno::PERM,
        }
    }

    pub(super) fn kill_recorded(pid: u32) -> std::io::Result<()> {
        let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
            return Ok(());
        };
        match kill_process_group(pid, Signal::KILL) {
            Ok(()) => Ok(()),
            Err(Errno::SRCH) => match kill_process(pid, Signal::KILL) {
                Ok(()) | Err(Errno::SRCH) => Ok(()),
                Err(e) => Err(e.into()),
            },
            Err(e) => Err(e.into()),
        }
    }

    /// Go's `syscall.Signal.String()` for the signals a supervised process meets; others by number.
    pub(super) fn signal_text(status: &std::process::ExitStatus) -> String {
        let Some(sig) = status.signal() else {
            return "unknown".into();
        };
        let word = match sig {
            1 => "hangup",
            2 => "interrupt",
            3 => "quit",
            6 if cfg!(target_os = "macos") => "abort trap",
            6 => "aborted",
            9 => "killed",
            11 => "segmentation fault",
            13 => "broken pipe",
            15 => "terminated",
            n => return format!("signal: signal {n}"),
        };
        format!("signal: {word}")
    }
}

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        TerminateProcess,
    };

    pub(super) fn alive(pid: u32) -> bool {
        // SAFETY: OpenProcess takes plain values and returns a handle or null; no pointers are passed.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            // Access denied still means it exists (another user's, elevated); no such pid means it does not.
            return std::io::Error::last_os_error().raw_os_error() == Some(5);
        }
        let mut code: u32 = 0;
        // SAFETY: `handle` is a valid process handle from OpenProcess and `code` is a live local the call writes.
        let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
        // SAFETY: `handle` was opened above and is closed exactly once.
        unsafe { CloseHandle(handle) };
        ok != 0 && code == u32::try_from(STILL_ACTIVE).unwrap_or(259)
    }

    pub(super) fn group_alive(_pid: u32) -> bool {
        // A tree's Job object ended every member when the daemon that held it died.
        false
    }

    pub(super) fn kill_recorded(pid: u32) -> std::io::Result<()> {
        // SAFETY: OpenProcess takes plain values and returns a handle or null; no pointers are passed.
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
        if handle.is_null() {
            // Gone already (invalid parameter) is not an error for a sweep.
            return Ok(());
        }
        // SAFETY: `handle` is a live process handle opened with PROCESS_TERMINATE.
        let ok = unsafe { TerminateProcess(handle, 1) };
        let err = (ok == 0).then(std::io::Error::last_os_error);
        // SAFETY: `handle` was opened above and is closed exactly once.
        unsafe { CloseHandle(handle) };
        err.map_or(Ok(()), Err)
    }

    pub(super) fn signal_text(_status: &std::process::ExitStatus) -> String {
        // Windows exits always carry a code; this is only reached for a status std could not read.
        "unknown".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_alive_and_an_exited_child_is_not() {
        assert!(alive(std::process::id()));
        assert!(!alive(0));
        let mut child = std::process::Command::new(std::env::current_exe().unwrap_or_default())
            .arg("--list")
            .stdout(std::process::Stdio::null())
            .spawn();
        if let Ok(c) = child.as_mut() {
            let pid = c.id();
            let _ = c.wait();
            assert!(!alive(pid), "a reaped child is gone");
            assert!(kill_recorded(pid).is_ok(), "a gone pid is not an error");
        }
    }

    #[test]
    fn exit_text_uses_gos_words() {
        let ok = std::process::Command::new(std::env::current_exe().unwrap_or_default())
            .arg("--list")
            .stdout(std::process::Stdio::null())
            .status();
        if let Ok(st) = ok {
            assert_eq!(exit_text(&st), "exited with status 0");
        }
    }
}
