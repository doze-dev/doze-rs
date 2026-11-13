//! Whether a process is still there, by pid: how the shared `.doze` registry decides a name's holder is gone (a
//! crashed process runs no shutdown hook, so liveness is by pid, not by a clean release).

/// Whether a process with this pid exists. On Unix a process owned by someone else counts (the signal-0 probe says
/// `EPERM`: it exists). On Windows a process that has exited does not count even while some handle keeps its pid
/// reserved (Go's `os.FindProcess` said it did).
pub fn alive(pid: u32) -> bool {
    pid != 0 && imp::alive(pid)
}

/// Asks a process to stop: SIGTERM on Unix, which it may catch and finish its work for. Windows has no such polite
/// word for a process doze did not start, so there it is refused.
pub fn terminate(pid: u32) -> std::io::Result<()> {
    if pid == 0 {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    imp::terminate(pid)
}

/// Stops a process at once: SIGKILL on Unix, which it cannot catch. Only ever the exact pid asked for; refused on
/// Windows, as [`terminate`] is.
pub fn kill(pid: u32) -> std::io::Result<()> {
    if pid == 0 {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    imp::kill(pid)
}

/// This process's user id: whose processes `ps -U` lists. 0 on Windows, which has none.
pub fn uid() -> u32 {
    imp::uid()
}

#[cfg(unix)]
mod imp {
    use rustix::io::Errno;
    use rustix::process::{Pid, Signal, kill_process, test_kill_process};

    pub(super) fn alive(pid: u32) -> bool {
        let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
            return false;
        };
        match test_kill_process(pid) {
            Ok(()) => true,
            Err(e) => e == Errno::PERM,
        }
    }

    pub(super) fn terminate(pid: u32) -> std::io::Result<()> {
        let pid = i32::try_from(pid)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        kill_process(pid, Signal::TERM).map_err(std::io::Error::from)
    }

    pub(super) fn kill(pid: u32) -> std::io::Result<()> {
        let pid = i32::try_from(pid)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        kill_process(pid, Signal::KILL).map_err(std::io::Error::from)
    }

    pub(super) fn uid() -> u32 {
        rustix::process::getuid().as_raw()
    }
}

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
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

    pub(super) fn terminate(_: u32) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    pub(super) fn kill(_: u32) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    pub(super) fn uid() -> u32 {
        0
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
        }
    }
}
