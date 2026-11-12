//! Windows: a single-instance named pipe with an unguessable name; the host checks that the client is the process
//! it started. A handle travels as the bytes of `WSADuplicateSocketW`, which the receiver turns back into a socket.
// Win32 calls: each unsafe block below does one thing and carries its SAFETY reason (p1.md).
#![allow(unsafe_code)]

use std::io;
use std::os::windows::io::{AsRawHandle, AsRawSocket, FromRawSocket, OwnedSocket};
use std::pin::Pin;
use std::process::Stdio;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf, ReadHalf, WriteHalf};
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Networking::WinSock::{
    FROM_PROTOCOL_INFO, INVALID_SOCKET, SOCKET, WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED,
    WSADuplicateSocketW, WSAGetLastError, WSAPROTOCOL_INFOW, WSASocketW,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};

pub(super) type Handle = OwnedSocket;
pub(super) const TRAILER_LEN: usize = std::mem::size_of::<WSAPROTOCOL_INFOW>();
const PIPE_ENV: &str = "DOZE_MODULE_PIPE";

#[derive(Debug)]
pub(super) enum Channel {
    Server(NamedPipeServer, u32),
    Client(NamedPipeClient, u32),
}

/// A name nobody can guess: 128 bits from two `RandomState`s, which std seeds from the OS random source.
fn pipe_name() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut a = std::collections::hash_map::RandomState::new().build_hasher();
    a.write_u32(std::process::id());
    let mut b = std::collections::hash_map::RandomState::new().build_hasher();
    b.write_u32(std::process::id().rotate_left(16));
    format!(
        r"\\.\pipe\doze-module-{:016x}{:016x}",
        a.finish(),
        b.finish()
    )
}

pub(super) async fn spawn_module(
    mut cmd: tokio::process::Command,
) -> io::Result<(tokio::process::Child, Channel)> {
    crate::winsock::init()?;
    let name = pipe_name();
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&name)?;
    cmd.env(PIPE_ENV, &name).stdin(Stdio::null());
    let child = cmd.spawn()?;
    let want = child.id().unwrap_or(0);
    tokio::time::timeout(Duration::from_secs(10), server.connect())
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "the module did not connect within 10 s",
            )
        })??;
    let mut got: u32 = 0;
    // SAFETY: the handle is the live pipe `server` owns; `got` is a valid out-pointer for the duration of the call.
    let ok = unsafe { GetNamedPipeClientProcessId(server.as_raw_handle() as HANDLE, &mut got) };
    if ok == 0 || got != want {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("pipe client is pid {got}, not the module {want}"),
        ));
    }
    Ok((child, Channel::Server(server, want)))
}

pub(super) async fn pair() -> io::Result<(Channel, Channel)> {
    crate::winsock::init()?;
    let name = pipe_name();
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&name)?;
    let client = ClientOptions::new().open(&name)?;
    server.connect().await?;
    let me = std::process::id();
    Ok((Channel::Server(server, me), Channel::Client(client, me)))
}

pub(super) fn module_channel() -> io::Result<Channel> {
    crate::winsock::init()?;
    let name = std::env::var(PIPE_ENV)
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, format!("{PIPE_ENV} is not set")))?;
    let client = ClientOptions::new().open(&name)?;
    let mut host: u32 = 0;
    // SAFETY: the handle is the live pipe `client` owns; `host` is a valid out-pointer for the duration of the call.
    let ok = unsafe { GetNamedPipeServerProcessId(client.as_raw_handle() as HANDLE, &mut host) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Channel::Client(client, host))
}

impl AsyncRead for Channel {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Channel::Server(p, _) => Pin::new(p).poll_read(cx, buf),
            Channel::Client(p, _) => Pin::new(p).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Channel {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Channel::Server(p, _) => Pin::new(p).poll_write(cx, buf),
            Channel::Client(p, _) => Pin::new(p).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Channel::Server(p, _) => Pin::new(p).poll_flush(cx),
            Channel::Client(p, _) => Pin::new(p).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Channel::Server(p, _) => Pin::new(p).poll_shutdown(cx),
            Channel::Client(p, _) => Pin::new(p).poll_shutdown(cx),
        }
    }
}

pub(super) fn split(c: Channel) -> (Reader, Writer) {
    let peer = match &c {
        Channel::Server(_, pid) | Channel::Client(_, pid) => *pid,
    };
    let (r, w) = tokio::io::split(c);
    (Reader { r }, Writer { w, peer })
}

#[derive(Debug)]
pub(super) struct Reader {
    r: ReadHalf<Channel>,
}

#[derive(Debug)]
pub(super) struct Writer {
    w: WriteHalf<Channel>,
    peer: u32,
}

impl Reader {
    pub(super) async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.r.read(buf).await {
            // The other side went away: a broken pipe means the same as end of file.
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(0),
            r => r,
        }
    }

    pub(super) fn import(&mut self, trailer: &[u8]) -> io::Result<OwnedSocket> {
        if trailer.len() != TRAILER_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "handle trailer of {} bytes, expected {TRAILER_LEN}",
                    trailer.len()
                ),
            ));
        }
        // SAFETY: WSAPROTOCOL_INFOW is plain data with no invalid bit patterns; the length was checked above and
        // read_unaligned copies it out of a possibly unaligned byte buffer.
        let info: WSAPROTOCOL_INFOW = unsafe { std::ptr::read_unaligned(trailer.as_ptr().cast()) };
        // SAFETY: `info` came from WSADuplicateSocketW naming this process; WSASocketW only reads it and returns a
        // new socket or INVALID_SOCKET.
        let s: SOCKET = unsafe {
            WSASocketW(
                FROM_PROTOCOL_INFO,
                FROM_PROTOCOL_INFO,
                FROM_PROTOCOL_INFO,
                &info,
                0,
                WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
            )
        };
        if s == INVALID_SOCKET {
            return Err(last_wsa_error());
        }
        // SAFETY: `s` is a socket WSASocketW just created for this process and nothing else owns it.
        Ok(unsafe { OwnedSocket::from_raw_socket(s as u64) })
    }
}

impl Writer {
    pub(super) fn export(&self, h: OwnedSocket) -> io::Result<(Vec<u8>, Option<OwnedSocket>)> {
        // SAFETY: an all-zero WSAPROTOCOL_INFOW is a valid value (integers, arrays and a GUID).
        let mut info: WSAPROTOCOL_INFOW = unsafe { std::mem::zeroed() };
        // SAFETY: `h` is a live socket we own; `info` is a valid out-pointer for the duration of the call.
        let r = unsafe { WSADuplicateSocketW(h.as_raw_socket() as SOCKET, self.peer, &mut info) };
        if r != 0 {
            return Err(last_wsa_error());
        }
        // SAFETY: viewing a plain-data struct as its bytes, for exactly its size, while `info` is alive.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&info as *const WSAPROTOCOL_INFOW).cast::<u8>(),
                TRAILER_LEN,
            )
        }
        .to_vec();
        // Our copy closes as `h` drops; the peer now has its own.
        Ok((bytes, None))
    }

    pub(super) async fn write(
        &mut self,
        bytes: &[u8],
        _ride: Option<OwnedSocket>,
    ) -> io::Result<()> {
        self.w.write_all(bytes).await
    }
}

fn last_wsa_error() -> io::Error {
    // SAFETY: WSAGetLastError reads this thread's last Winsock error; it has no preconditions.
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}
