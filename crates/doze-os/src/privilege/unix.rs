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

fn check(rc: libc::c_int, what: &str) -> io::Result<()> {
    if rc == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    Err(io::Error::new(e.kind(), format!("drop_root: {what}: {e}")))
}
