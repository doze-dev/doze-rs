//! The control channel between doze and a module process: a byte stream in both directions that can also carry a
//! client connection (a [`Passable`]) to the other side.
//!
//! The host starts the module with [`spawn_module`]; the module, first thing, calls [`module_channel`]. Either side
//! then splits the channel into a [`Reader`] and a [`Writer`]. The channel ends (reads return 0) when the other side
//! exits for any reason, including being killed: that is how a module notices its host is gone.
//!
//! Handing over a connection: the sender calls [`Writer::export`], appends [`Exported::trailer`] to the frame it is
//! about to write (it is empty on Unix and [`TRAILER_LEN`] bytes on Windows), and writes the frame with
//! [`Writer::write_exported`]. The receiver, reading a frame it knows carries a handle, splits the last
//! [`TRAILER_LEN`] bytes off and calls [`Reader::import`]. Frames themselves are the caller's business.
//!
//! The sender keeps a [`Passable::hold`] until the other side has shown it has the handle: an answer to the frame
//! that carried it, or anything sent only after reading that frame. A handle whose only reference is the message
//! carrying it can be shut on the way. macOS's collector of descriptors in flight (`unp_gc`, run whenever any process
//! closes a Unix socket while descriptors are in flight) can count it unreachable in the moment the receiver's
//! `recvmsg` has taken the message off the channel but not yet installed the descriptor, and `sorflush`es it (the
//! window is read from XNU's `uipc_usrreq.c`). The receiver then holds the right socket, peer address and all, but
//! its first read returns 0 and the client's bytes are gone. Measured: about 1 in 60 hand-overs while another thread
//! closes such sockets, none with the sender holding a copy (`doze-module-proto` `tests/handoff.rs`).

use std::io;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

/// How many bytes a handle adds to the frame that carries it.
pub const TRAILER_LEN: usize = imp::TRAILER_LEN;

/// The control channel, before it is split.
#[derive(Debug)]
pub struct Channel(imp::Channel);

/// Starts `cmd` as a module connected to a new channel. On Unix the channel is the module's stdin; on Windows it is
/// a named pipe whose name the module finds in its environment, and the module's stdin is null. The caller sets
/// everything else about `cmd` (arguments, environment, stdout and stderr).
pub async fn spawn_module(
    cmd: tokio::process::Command,
) -> io::Result<(tokio::process::Child, Channel)> {
    let (child, ch) = imp::spawn_module(cmd).await?;
    Ok((child, Channel(ch)))
}

/// Both ends of a channel inside this process: a module run in-process (tests, an embedded module). Handles can be
/// passed across it as across a real one.
pub async fn pair() -> io::Result<(Channel, Channel)> {
    let (a, b) = imp::pair().await?;
    Ok((Channel(a), Channel(b)))
}

/// The module's end of the channel. Call it once, before starting any other process: on Unix it also points the
/// module's stdin at the null device, so nothing the module runs inherits the channel.
pub fn module_channel() -> io::Result<Channel> {
    imp::module_channel().map(Channel)
}

impl Channel {
    /// Splits the channel so reading and writing can happen on different tasks.
    pub fn split(self) -> (Reader, Writer) {
        let (r, w) = imp::split(self.0);
        (Reader(r), Writer(w))
    }
}

/// The reading half of a [`Channel`].
#[derive(Debug)]
pub struct Reader(imp::Reader);

/// The writing half of a [`Channel`].
#[derive(Debug)]
pub struct Writer(imp::Writer);

impl Reader {
    /// Reads some bytes. `Ok(0)` means the other side has gone.
    pub async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf).await
    }

    /// Takes the handle that arrived with a frame, given the frame's trailer (the last [`TRAILER_LEN`] bytes).
    pub fn import(&mut self, trailer: &[u8]) -> io::Result<Passable> {
        self.0.import(trailer).map(Passable)
    }
}

impl Writer {
    /// Writes all of `bytes`.
    pub async fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.write(bytes, None).await
    }

    /// Prepares `handle` to travel to the other side. The handle is consumed: from here on it belongs to the frame.
    /// Take a [`Passable::hold`] first and keep it until the other side shows it has the handle (module docs).
    pub fn export(&self, handle: Passable) -> io::Result<Exported> {
        let (trailer, ride) = self.0.export(handle.0)?;
        Ok(Exported { trailer, ride })
    }

    /// Writes one frame that carries `exported`. The frame must end with `exported.trailer()`.
    pub async fn write_exported(&mut self, frame: &[u8], exported: Exported) -> io::Result<()> {
        self.0.write(frame, exported.ride).await
    }
}

/// A handle on its way to the other side of a channel.
#[derive(Debug)]
pub struct Exported {
    trailer: Vec<u8>,
    ride: Option<imp::Handle>,
}

impl Exported {
    /// The bytes to append to the frame that carries the handle.
    pub fn trailer(&self) -> &[u8] {
        &self.trailer
    }
}

/// A connection that can be handed to the other side of a channel: a TCP stream (and on Unix, a Unix stream).
#[derive(Debug)]
pub struct Passable(imp::Handle);

impl Passable {
    /// A copy of the connection for the sender to keep while the handle is in flight, so it is never the message's
    /// alone (module docs). Dropping it closes only the copy: the connection lives on with the other side.
    pub fn hold(&self) -> io::Result<Held> {
        self.0.try_clone().map(|c| Held { _copy: c })
    }
}

/// The sender's copy of a connection it handed over; see [`Passable::hold`].
#[derive(Debug)]
pub struct Held {
    _copy: imp::Handle,
}

impl From<std::net::TcpStream> for Passable {
    fn from(s: std::net::TcpStream) -> Passable {
        Passable(imp::Handle::from(s))
    }
}

impl From<Passable> for std::net::TcpStream {
    fn from(p: Passable) -> std::net::TcpStream {
        std::net::TcpStream::from(p.0)
    }
}

#[cfg(unix)]
impl From<std::os::unix::net::UnixStream> for Passable {
    fn from(s: std::os::unix::net::UnixStream) -> Passable {
        Passable(imp::Handle::from(s))
    }
}

#[cfg(unix)]
impl From<Passable> for std::os::unix::net::UnixStream {
    fn from(p: Passable) -> std::os::unix::net::UnixStream {
        std::os::unix::net::UnixStream::from(p.0)
    }
}
