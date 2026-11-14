//! Ctrl-C and SIGTERM as something a blocking program can look at: [`Interrupt::watch`] takes them over (so the
//! process is not killed by them) and from then on [`Interrupt::raised`] says whether one came. A command that follows
//! a stream (`doze logs -f`, `doze status -w`) watches, checks between frames, and ends the way a follow ends: with
//! its last line written and exit 0. Unix: SIGINT and SIGTERM. Windows: Ctrl-C and Ctrl-Break.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

/// Whether an interrupt has come since [`Interrupt::watch`].
#[derive(Debug, Clone)]
pub struct Interrupt(Arc<AtomicBool>);

impl Interrupt {
    /// Starts watching on a thread of its own. The signals no longer end the process: the caller checks
    /// [`Interrupt::raised`] and ends itself.
    pub fn watch() -> io::Result<Interrupt> {
        let flag = Arc::new(AtomicBool::new(false));
        let theirs = Arc::clone(&flag);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()?;
        // The handlers are registered before this returns, so a signal that comes at once is not lost.
        let waiter = rt.block_on(async { Waiter::new() })?;
        thread::Builder::new()
            .name("doze-interrupt".into())
            .spawn(move || {
                rt.block_on(waiter.wait());
                theirs.store(true, Ordering::SeqCst);
            })?;
        Ok(Interrupt(flag))
    }

    /// One that is never raised (for tests, and where nothing should stop a follow but its end).
    pub fn never() -> Interrupt {
        Interrupt(Arc::new(AtomicBool::new(false)))
    }

    /// Whether one came.
    pub fn raised(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Raises it by hand, as a signal would.
    pub fn raise(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[cfg(unix)]
struct Waiter {
    int: tokio::signal::unix::Signal,
    term: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Waiter {
    fn new() -> io::Result<Waiter> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Waiter {
            int: signal(SignalKind::interrupt())?,
            term: signal(SignalKind::terminate())?,
        })
    }

    async fn wait(mut self) {
        tokio::select! {
            _ = self.int.recv() => {}
            _ = self.term.recv() => {}
        }
    }
}

#[cfg(windows)]
struct Waiter {
    c: tokio::signal::windows::CtrlC,
    brk: tokio::signal::windows::CtrlBreak,
}

#[cfg(windows)]
impl Waiter {
    fn new() -> io::Result<Waiter> {
        Ok(Waiter {
            c: tokio::signal::windows::ctrl_c()?,
            brk: tokio::signal::windows::ctrl_break()?,
        })
    }

    async fn wait(mut self) {
        tokio::select! {
            _ = self.c.recv() => {}
            _ = self.brk.recv() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raised_interrupt_is_seen_by_every_copy() {
        let i = Interrupt::never();
        let copy = i.clone();
        assert!(!copy.raised());
        i.raise();
        assert!(copy.raised());
    }

    #[cfg(unix)]
    #[test]
    fn sigterm_is_caught_not_fatal() {
        let i = Interrupt::watch().unwrap();
        rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::TERM)
            .unwrap();
        for _ in 0..200 {
            if i.raised() {
                return;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("SIGTERM was not seen");
    }
}
