//! Unix: setgroups, setgid, setuid, in that order (each later call needs the rights the earlier ones still have).
#![allow(unsafe_code)]

use std::io;

pub(super) fn drop_root(uid: u32, gid: u32) -> io::Result<()> {
    let groups = [gid];
    // SAFETY: plain libc calls with a valid pointer and length; they change this process's credentials and touch no
    // memory of ours. Called before any other thread exists (the caller's contract), so no thread keeps root.
    let rc = unsafe { libc::setgroups(1, groups.as_ptr()) };
    check(rc, "setgroups")?;
    // SAFETY: as above.
    check(unsafe { libc::setgid(gid) }, "setgid")?;
    // SAFETY: as above.
    check(unsafe { libc::setuid(uid) }, "setuid")?;
    // A process that can become root again has not dropped it.
    // SAFETY: as above; on success this undoes the drop, which is the failure being checked for.
    if unsafe { libc::setuid(0) } == 0 {
        return Err(io::Error::other("drop_root: root could be taken back"));
    }
    Ok(())
}

pub(super) fn ids() -> (u32, u32) {
    (
        rustix::process::getuid().as_raw(),
        rustix::process::getgid().as_raw(),
    )
}

/// The home directory the user database gives `uid`.
pub(super) fn home_of(uid: u32) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: a zeroed passwd is a valid value for getpwuid_r to fill (plain integers and null pointers).
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is to memory of ours that outlives the call, and the length is the buffer's own; on
    // success pwd's strings point into buf, which is read below while still alive.
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            &raw mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &raw mut found,
        )
    };
    if rc != 0 || found.is_null() || pwd.pw_dir.is_null() {
        return None;
    }
    // SAFETY: getpwuid_r succeeded, so pw_dir is a NUL-terminated string inside buf.
    let dir = unsafe { std::ffi::CStr::from_ptr(pwd.pw_dir) };
    let path = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes()));
    (!path.as_os_str().is_empty()).then_some(path)
}

fn check(rc: libc::c_int, what: &str) -> io::Result<()> {
    if rc == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    Err(io::Error::new(e.kind(), format!("drop_root: {what}: {e}")))
}
