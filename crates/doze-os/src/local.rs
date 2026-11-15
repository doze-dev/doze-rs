//! The daemon's control socket as plain blocking I/O: a Unix socket where the system has them. A client thread reads
//! it with no async runtime (docs/port/app/PLAN.md §4.3). Windows gets a named pipe once the core has one; until then
//! connecting or listening there is an `Unsupported` error, and the types cannot be made.

use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

/// Nothing: on a system without a local socket the stream and listener cannot exist.
#[cfg(not(unix))]
#[derive(Debug)]
enum Never {}

/// A connected local socket, read and written with blocking calls.
#[derive(Debug)]
pub struct LocalStream {
    #[cfg(unix)]
    inner: UnixStream,
    #[cfg(not(unix))]
    inner: Never,
}

/// A listening local socket.
#[derive(Debug)]
pub struct LocalListener {
    #[cfg(unix)]
    inner: UnixListener,
    #[cfg(not(unix))]
    inner: Never,
}

#[cfg(not(unix))]
fn unsupported(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "{}: this system has no local control socket yet",
            path.display()
        ),
    )
}

impl LocalStream {
    /// Connects to the socket at `path`.
    pub fn connect(path: &Path) -> io::Result<LocalStream> {
        #[cfg(unix)]
        {
            UnixStream::connect(path).map(|inner| LocalStream { inner })
        }
        #[cfg(not(unix))]
        {
            Err(unsupported(path))
        }
    }

    /// A second handle on the same connection: one thread reads while another writes.
    pub fn try_clone(&self) -> io::Result<LocalStream> {
        #[cfg(unix)]
        {
            self.inner.try_clone().map(|inner| LocalStream { inner })
        }
        #[cfg(not(unix))]
        match self.inner {}
    }

    /// Ends the connection both ways. A thread blocked reading it wakes with end of file.
    pub fn shutdown(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            match self.inner.shutdown(std::net::Shutdown::Both) {
                // Already closed by the other side: nothing left to end.
                Err(e) if e.kind() == io::ErrorKind::NotConnected => Ok(()),
                other => other,
            }
        }
        #[cfg(not(unix))]
        match self.inner {}
    }

    /// How long a read waits before failing with `WouldBlock` or `TimedOut` (`None`: for ever).
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.inner.set_read_timeout(timeout)
        }
        #[cfg(not(unix))]
        {
            let _ = timeout;
            match self.inner {}
        }
    }
}

impl Read for LocalStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            self.inner.read(buf)
        }
        #[cfg(not(unix))]
        {
            let _ = buf;
            match self.inner {}
        }
    }
}

impl Write for LocalStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            self.inner.write(buf)
        }
        #[cfg(not(unix))]
        {
            let _ = buf;
            match self.inner {}
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.inner.flush()
        }
        #[cfg(not(unix))]
        match self.inner {}
    }
}

impl LocalListener {
    /// Listens at `path`, replacing a socket file a previous listener left behind (as the Go daemon does).
    pub fn bind(path: &Path) -> io::Result<LocalListener> {
        #[cfg(unix)]
        {
            match std::fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
            UnixListener::bind(path).map(|inner| LocalListener { inner })
        }
        #[cfg(not(unix))]
        {
            Err(unsupported(path))
        }
    }

    /// Waits for the next client.
    pub fn accept(&self) -> io::Result<LocalStream> {
        #[cfg(unix)]
        {
            self.inner.accept().map(|(inner, _)| LocalStream { inner })
        }
        #[cfg(not(unix))]
        match self.inner {}
    }
}
