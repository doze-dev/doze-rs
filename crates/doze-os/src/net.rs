//! Dialing an engine's backend: a Unix socket where the system has them, loopback TCP everywhere, behind one stream
//! type, so a module that asks its engine something (`INFO`, a health check) and the core that dials a backend never
//! branch on the operating system themselves.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;

/// A connected stream to a backend.
#[derive(Debug)]
pub enum Stream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

/// Connects to a Unix socket. Where there are none (Windows), an `Unsupported` error: the host grants such systems
/// TCP backends (PLAN.md D13), so this is only met by a caller that ignored the grant.
pub async fn connect_unix(path: &str) -> io::Result<Stream> {
    #[cfg(unix)]
    {
        UnixStream::connect(path).await.map(Stream::Unix)
    }
    #[cfg(not(unix))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{path}: this system has no Unix sockets"),
        ))
    }
}

/// Connects to `host:port`.
pub async fn connect_tcp(addr: &str) -> io::Result<Stream> {
    TcpStream::connect(addr).await.map(Stream::Tcp)
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Tcp(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(unix)]
            Stream::Unix(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Tcp(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(unix)]
            Stream::Unix(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Tcp(s) => Pin::new(s).poll_flush(cx),
            #[cfg(unix)]
            Stream::Unix(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Tcp(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(unix)]
            Stream::Unix(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}
