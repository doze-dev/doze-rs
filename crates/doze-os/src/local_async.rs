//! The daemon's side of the control socket on its tokio runtime: the listener accepts as a task, and each connection
//! splits into a read half and a write half that tasks own, so no connection, request or stream costs a thread. The
//! same socket as [`crate::local`] (a Unix socket where the system has them); on Windows binding is an `Unsupported`
//! error, as there, and the types cannot be made.

use std::io;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[cfg(unix)]
use tokio::net::{
    UnixListener, UnixStream,
    unix::{OwnedReadHalf, OwnedWriteHalf},
};

/// Nothing: on a system without a local socket the listener and the halves cannot exist.
#[cfg(not(unix))]
#[derive(Debug)]
enum Never {}

/// A listening local socket, accepted from on a tokio runtime.
#[derive(Debug)]
pub struct Listener {
    #[cfg(unix)]
    inner: UnixListener,
    #[cfg(not(unix))]
    inner: Never,
}

/// A connection's read half.
#[derive(Debug)]
pub struct ReadHalf {
    #[cfg(unix)]
    inner: OwnedReadHalf,
    #[cfg(not(unix))]
    inner: Never,
}

/// A connection's write half; dropping it ends the connection's writing direction.
#[derive(Debug)]
pub struct WriteHalf {
    #[cfg(unix)]
    inner: OwnedWriteHalf,
    #[cfg(not(unix))]
    inner: Never,
}

impl Listener {
    /// Listens at `path`, replacing a socket file a previous listener left behind. Call within a tokio runtime.
    pub fn bind(path: &Path) -> io::Result<Listener> {
        #[cfg(unix)]
        {
            match std::fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
            UnixListener::bind(path).map(|inner| Listener { inner })
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "{}: this system has no local control socket yet",
                    path.display()
                ),
            ))
        }
    }

    /// Waits for the next client: its read and write halves.
    pub async fn accept(&self) -> io::Result<(ReadHalf, WriteHalf)> {
        #[cfg(unix)]
        {
            let (s, _) = self.inner.accept().await?;
            Ok(split(s))
        }
        #[cfg(not(unix))]
        match self.inner {}
    }
}

#[cfg(unix)]
fn split(s: UnixStream) -> (ReadHalf, WriteHalf) {
    let (r, w) = s.into_split();
    (ReadHalf { inner: r }, WriteHalf { inner: w })
}

impl AsyncRead for ReadHalf {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        #[cfg(unix)]
        {
            Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
        }
        #[cfg(not(unix))]
        {
            let _ = (cx, buf);
            match self.get_mut().inner {}
        }
    }
}

impl AsyncWrite for WriteHalf {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        #[cfg(unix)]
        {
            Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
        }
        #[cfg(not(unix))]
        {
            let _ = (cx, buf);
            match self.get_mut().inner {}
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        #[cfg(unix)]
        {
            Pin::new(&mut self.get_mut().inner).poll_flush(cx)
        }
        #[cfg(not(unix))]
        {
            let _ = cx;
            match self.get_mut().inner {}
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        #[cfg(unix)]
        {
            Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
        }
        #[cfg(not(unix))]
        {
            let _ = cx;
            match self.get_mut().inner {}
        }
    }
}
