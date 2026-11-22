//! Being asked to stop: SIGTERM or SIGINT on Unix, CTRL_BREAK or CTRL_C on Windows ([`crate::tree::Stop`] sends
//! these). The daemon waits on it to shut down gracefully; test helpers use it to play a well-behaved process.

use std::io;

/// Waits until this process is asked to stop. The handlers are installed when this is first polled.
pub async fn stop_requested() -> io::Result<()> {
    install()?.requested().await
}

/// The stop handlers, installed now: from here on a stop request is caught and kept until [`Stop::requested`] sees
/// it, instead of ending the process. A daemon installs them before anyone can find it (its socket, its pid file), so
/// a stop that comes at once is still a graceful one.
pub fn install() -> io::Result<Stop> {
    imp::Handlers::install().map(Stop)
}

/// Stop handlers already installed ([`install`]).
pub struct Stop(imp::Handlers);

impl std::fmt::Debug for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Stop")
    }
}

impl Stop {
    /// Waits until this process is asked to stop (or was asked since [`install`]).
    pub async fn requested(self) -> io::Result<()> {
        self.0.wait().await;
        Ok(())
    }
}

#[cfg(unix)]
mod imp {
    use tokio::signal::unix::{Signal, SignalKind, signal};

    pub(crate) struct Handlers {
        term: Signal,
        int: Signal,
    }

    impl Handlers {
        pub(crate) fn install() -> std::io::Result<Handlers> {
            Ok(Handlers {
                term: signal(SignalKind::terminate())?,
                int: signal(SignalKind::interrupt())?,
            })
        }

        pub(crate) async fn wait(mut self) {
            tokio::select! {
                _ = self.term.recv() => {},
                _ = self.int.recv() => {},
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use tokio::signal::windows::{CtrlBreak, CtrlC, ctrl_break, ctrl_c};

    pub(crate) struct Handlers {
        brk: CtrlBreak,
        c: CtrlC,
    }

    impl Handlers {
        pub(crate) fn install() -> std::io::Result<Handlers> {
            Ok(Handlers {
                brk: ctrl_break()?,
                c: ctrl_c()?,
            })
        }

        pub(crate) async fn wait(mut self) {
            tokio::select! {
                _ = self.brk.recv() => {},
                _ = self.c.recv() => {},
            }
        }
    }
}
