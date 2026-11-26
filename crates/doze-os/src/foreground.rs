//! Running a command in the foreground and waiting for it, the way a job-control shell runs one (`doze run`,
//! `doze connect`): the command's signals are the command's, and doze reports how it ended.
//!
//! Unix: the command gets a process group of its own, and the terminal when doze has it (`tcsetpgrp`), so Ctrl-C,
//! Ctrl-\ and a hang-up from the terminal reach the command alone (a database shell cancels its query and stays). A
//! SIGINT, SIGTERM, SIGHUP or SIGQUIT sent to doze by another process (an IDE's stop, `kill -INT <doze>`, or the
//! whole group doze is in) is passed on to the command's group, once, and doze goes on waiting for it; one that comes
//! while the command is starting is passed on as soon as it has started (it does not end doze). Without the
//! group of its own, a signal sent to doze alone never reached the command, and one sent to doze's group reached it
//! from both sides. Ctrl-Z stops the command, then doze with it (its shell sees the job stopped); `fg` gives the
//! terminal back to the command and continues it. When the command ends, doze takes the terminal back.
//!
//! Windows: Ctrl-C is the console's, sent to every process on it: doze takes it (so it does not end doze) and waits.

use std::io;
use std::process::{Command, ExitStatus};

/// Runs `cmd` in the foreground, as above, and gives how it ended. One at a time per process.
pub fn run(cmd: Command) -> io::Result<ExitStatus> {
    imp::run(cmd)
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::process::{Command, ExitStatus};

    pub(super) fn run(mut cmd: Command) -> io::Result<ExitStatus> {
        let _ = crate::interrupt::pass_to_child();
        cmd.status()
    }
}

#[cfg(unix)]
#[allow(unsafe_code)] // the signal handlers and the signal mask: each block says why it is sound
mod imp {
    use rustix::fd::{AsFd, BorrowedFd};
    use rustix::process::{Pid, Signal, WaitOptions};
    use std::io;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::{Command, ExitStatus};
    use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

    /// The process group the signals doze is sent are passed to: the command's (its pid), 0 while none runs.
    static GROUP: AtomicI32 = AtomicI32::new(0);

    /// The signals caught before `GROUP` was known (one bit each, by their place in `PASSED`), not yet passed on.
    static PENDING: AtomicU32 = AtomicU32::new(0);

    /// The signals passed on to the command.
    const PASSED: [libc::c_int; 4] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT];

    fn bit(sig: libc::c_int) -> u32 {
        PASSED.iter().position(|&s| s == sig).map_or(0, |i| 1 << i)
    }

    /// The handler. It marks the signal pending, then passes it on if the group is known and the mark is still there:
    /// whichever of it and `run` (which takes the marks once it knows the group) clears a mark passes that signal on,
    /// so it goes once, however the two interleave.
    extern "C" fn pass_on(sig: libc::c_int) {
        let bit = bit(sig);
        PENDING.fetch_or(bit, Ordering::SeqCst);
        let group = GROUP.load(Ordering::SeqCst);
        if group > 0 && PENDING.fetch_and(!bit, Ordering::SeqCst) & bit != 0 {
            // SAFETY: kill(2) is async-signal-safe; it only reads its two integers.
            unsafe { libc::kill(-group, sig) };
        }
    }

    pub(super) fn run(mut cmd: Command) -> io::Result<ExitStatus> {
        let tty = foreground_tty();
        cmd.process_group(0);
        // The handlers go in before the command starts: a stop sent in between would otherwise end doze (the default
        // action) and leave the command running without it. One caught before the group is known waits in `PENDING`.
        // A signal doze was started ignoring (`nohup`'s SIGHUP) gets its handler after: the command starts ignoring it
        // too, as a shell's job does.
        PENDING.store(0, Ordering::SeqCst);
        let saved = Handlers::install(false);
        let child = cmd.spawn()?;
        let pid = i32::try_from(child.id()).map_err(io::Error::other)?;
        let group = Pid::from_raw(pid).ok_or_else(|| io::Error::other("pid 0"))?;
        // The child is reaped below (waitpid), not through `child`.
        drop(child);
        if let Some(fd) = tty {
            give(fd, group);
        }
        GROUP.store(pid, Ordering::SeqCst);
        let ignored = Handlers::install(true);
        let caught = PENDING.swap(0, Ordering::SeqCst);
        for sig in PASSED.into_iter().filter(|&s| caught & bit(s) != 0) {
            // SAFETY: as in `pass_on`.
            unsafe { libc::kill(-pid, sig) };
        }
        let status = wait(group, tty);
        GROUP.store(0, Ordering::SeqCst);
        drop((saved, ignored));
        if let Some(fd) = tty {
            give(fd, rustix::process::getpgrp());
        }
        status
    }

    /// Waits for the command to end; a stop is job control's (above).
    fn wait(group: Pid, tty: Option<BorrowedFd<'static>>) -> io::Result<ExitStatus> {
        loop {
            let status = match rustix::process::waitpid(Some(group), WaitOptions::UNTRACED) {
                Ok(Some((_, s))) => s,
                Ok(None) => continue,
                Err(rustix::io::Errno::INTR) => continue,
                Err(e) => return Err(e.into()),
            };
            if !status.stopped() {
                return Ok(ExitStatus::from_raw(status.as_raw()));
            }
            let Some(fd) = tty else {
                continue; // no terminal, no job control: whoever stopped it continues it
            };
            let ours = rustix::process::getpgrp();
            let read_too_soon = matches!(
                status.stopping_signal(),
                Some(s) if s == libc::SIGTTIN || s == libc::SIGTTOU
            ) && rustix::termios::tcgetpgrp(fd).ok() == Some(group);
            if !read_too_soon {
                // Ctrl-Z: doze stops with the command, as one job; it goes on here when continued.
                give(fd, ours);
                let _ = rustix::process::kill_process(rustix::process::getpid(), Signal::TSTP);
                if rustix::termios::tcgetpgrp(fd).ok() == Some(ours) {
                    give(fd, group);
                }
            }
            // (A read that came before the terminal was handed over stopped it: it is the command's now.)
            let _ = rustix::process::kill_process_group(group, Signal::CONT);
        }
    }

    /// The terminal doze has in the foreground (its stdin, stdout or stderr), if any.
    fn foreground_tty() -> Option<BorrowedFd<'static>> {
        let ours = rustix::process::getpgrp();
        [
            rustix::stdio::stdin(),
            rustix::stdio::stdout(),
            rustix::stdio::stderr(),
        ]
        .into_iter()
        .find(|fd| rustix::termios::isatty(fd) && rustix::termios::tcgetpgrp(fd).ok() == Some(ours))
    }

    /// Makes `group` the terminal's foreground group. Doze may be in the background when it does (taking the terminal
    /// back), so SIGTTOU, which would stop it, is held off for the call.
    fn give(fd: BorrowedFd<'_>, group: Pid) {
        let held = Mask::hold(libc::SIGTTOU);
        let _ = rustix::termios::tcsetpgrp(fd.as_fd(), group);
        drop(held);
    }

    /// This thread's signal mask with one more signal blocked, until dropped.
    struct Mask(libc::sigset_t);

    impl Mask {
        fn hold(sig: libc::c_int) -> Mask {
            let mut set = empty_set();
            // SAFETY: `set` is an initialised sigset_t this function owns.
            unsafe { libc::sigaddset(&raw mut set, sig) };
            let mut old = empty_set();
            // SAFETY: both pointers are to sigset_t values owned here; it changes this thread's mask only.
            unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &raw const set, &raw mut old) };
            Mask(old)
        }
    }

    impl Drop for Mask {
        fn drop(&mut self) {
            // SAFETY: restores the mask saved by `hold` on this thread; the pointer is to that saved value.
            unsafe {
                libc::pthread_sigmask(libc::SIG_SETMASK, &raw const self.0, std::ptr::null_mut())
            };
        }
    }

    fn empty_set() -> libc::sigset_t {
        // SAFETY: sigset_t is plain data; all zeroes is a valid value, and sigemptyset below makes it the empty set.
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: the pointer is to the local above.
        unsafe { libc::sigemptyset(&raw mut set) };
        set
    }

    /// The handlers that were there before, put back when dropped (tokio's, for a command that watched for Ctrl-C).
    struct Handlers(Vec<(libc::c_int, libc::sigaction)>);

    impl Handlers {
        /// Puts `pass_on` in for the passed signals doze ignores (`ignored`) or the others.
        fn install(ignored: bool) -> Handlers {
            // SAFETY: sigaction is plain data; all zeroes is a valid value, filled in below.
            let mut act: libc::sigaction = unsafe { std::mem::zeroed() };
            act.sa_sigaction = pass_on as extern "C" fn(libc::c_int) as libc::sighandler_t;
            act.sa_flags = libc::SA_RESTART;
            act.sa_mask = empty_set();
            let mut saved = Vec::with_capacity(PASSED.len());
            for sig in PASSED {
                // SAFETY: as above.
                let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
                // SAFETY: reads the action only; the pointer is to the local above.
                if unsafe { libc::sigaction(sig, std::ptr::null(), &raw mut old) } != 0
                    || (old.sa_sigaction == libc::SIG_IGN) != ignored
                {
                    continue;
                }
                // SAFETY: `pass_on` only does async-signal-safe work; both pointers are to locals.
                if unsafe { libc::sigaction(sig, &raw const act, &raw mut old) } == 0 {
                    saved.push((sig, old));
                }
            }
            Handlers(saved)
        }
    }

    impl Drop for Handlers {
        fn drop(&mut self) {
            for (sig, old) in &self.0 {
                // SAFETY: puts back the action `install` read for this signal; the pointer is to that saved value.
                unsafe { libc::sigaction(*sig, old, std::ptr::null_mut()) };
            }
        }
    }
}
