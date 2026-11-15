//! The transport under the client. A [`Dialer`] opens a [`Conn`]; the client never names the socket kind, so the
//! Windows named pipe (once the daemon has one) is a new dialer, not a new client.

use doze_os::local::LocalStream;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One open connection to the daemon: a byte stream that one thread can read while another writes.
pub trait Conn: Read + Write + Send + Sync + fmt::Debug + 'static {
    /// A second handle on the same connection.
    fn try_clone_conn(&self) -> io::Result<Box<dyn Conn>>;
    /// Ends the connection both ways; a blocked reader wakes with end of file.
    fn shutdown(&self) -> io::Result<()>;
    /// How long a read may wait (`None`: for ever).
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
}

/// Opens connections to one daemon.
pub trait Dialer: Send + Sync + fmt::Debug + 'static {
    /// A new connection.
    fn dial(&self) -> io::Result<Box<dyn Conn>>;
}

impl Conn for LocalStream {
    fn try_clone_conn(&self) -> io::Result<Box<dyn Conn>> {
        Ok(Box::new(self.try_clone()?))
    }

    fn shutdown(&self) -> io::Result<()> {
        LocalStream::shutdown(self)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        LocalStream::set_read_timeout(self, timeout)
    }
}

/// Dials the daemon's control socket (`<project>/run/doze.admin.sock`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketDialer {
    path: PathBuf,
}

impl SocketDialer {
    /// A dialer for the socket at `path`.
    pub fn new(path: impl Into<PathBuf>) -> SocketDialer {
        SocketDialer { path: path.into() }
    }

    /// The socket's path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Dialer for SocketDialer {
    fn dial(&self) -> io::Result<Box<dyn Conn>> {
        Ok(Box::new(LocalStream::connect(&self.path)?))
    }
}
