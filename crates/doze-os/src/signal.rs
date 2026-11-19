//! Being asked to stop: SIGTERM or SIGINT on Unix, CTRL_BREAK or CTRL_C on Windows ([`crate::tree::Stop`] sends
//! these). The daemon waits on it to shut down gracefully; test helpers use it to play a well-behaved process.

use std::io;

/// Waits until this process is asked to stop. The handlers are installed when this is first polled.
pub async fn stop_requested() -> io::Result<()> {
    imp::stop_requested().await
}

#[cfg(unix)]
mod imp {
    use tokio::signal::unix::{SignalKind, signal};

    pub(super) async fn stop_requested() -> std::io::Result<()> {
        let mut term = signal(SignalKind::terminate())?;
        let mut int = signal(SignalKind::interrupt())?;
        tokio::select! {
            _ = term.recv() => {},
            _ = int.recv() => {},
        }
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    pub(super) async fn stop_requested() -> std::io::Result<()> {
        let mut brk = tokio::signal::windows::ctrl_break()?;
        let mut c = tokio::signal::windows::ctrl_c()?;
        tokio::select! {
            _ = brk.recv() => {},
            _ = c.recv() => {},
        }
        Ok(())
    }
}
