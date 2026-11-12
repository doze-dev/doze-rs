//! Unix: a socketpair whose module end is the module's stdin; handles travel as SCM_RIGHTS on the same socket.

use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags,
};
use std::collections::VecDeque;
use std::io::{self, IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::Interest;
use tokio::net::UnixStream;

pub(super) type Handle = OwnedFd;
pub(super) const TRAILER_LEN: usize = 0;

#[derive(Debug)]
pub(super) struct Channel(UnixStream);

pub(super) async fn spawn_module(
    mut cmd: tokio::process::Command,
) -> io::Result<(tokio::process::Child, Channel)> {
    let (ours, theirs) = std::os::unix::net::UnixStream::pair()?;
    cmd.stdin(Stdio::from(OwnedFd::from(theirs)));
    let child = cmd.spawn()?;
    ours.set_nonblocking(true)?;
    Ok((child, Channel(UnixStream::from_std(ours)?)))
}

pub(super) fn module_channel() -> io::Result<Channel> {
    // Our own copy of the socket (close-on-exec), then fd 0 becomes /dev/null so programs this module runs
    // (psql, mariadb …) never inherit the channel (S0 measured a grandchild holding it otherwise).
    let ctl = std::io::stdin().as_fd().try_clone_to_owned()?;
    let null = rustix::fs::open(
        "/dev/null",
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    rustix::stdio::dup2_stdin(&null)?;
    let s = std::os::unix::net::UnixStream::from(ctl);
    s.set_nonblocking(true)?;
    Ok(Channel(UnixStream::from_std(s)?))
}

pub(super) fn split(c: Channel) -> (Reader, Writer) {
    let s = Arc::new(c.0);
    (
        Reader {
            s: s.clone(),
            fds: VecDeque::new(),
        },
        Writer { s },
    )
}

#[derive(Debug)]
pub(super) struct Reader {
    s: Arc<UnixStream>,
    fds: VecDeque<OwnedFd>,
}

#[derive(Debug)]
pub(super) struct Writer {
    s: Arc<UnixStream>,
}

impl Reader {
    pub(super) async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut got: Vec<OwnedFd> = Vec::new();
        let s = &self.s;
        let n = s
            .async_io(Interest::READABLE, || {
                let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
                let mut anc = RecvAncillaryBuffer::new(&mut space);
                let r = rustix::net::recvmsg(
                    &**s,
                    &mut [IoSliceMut::new(buf)],
                    &mut anc,
                    recv_flags(),
                )?;
                for m in anc.drain() {
                    if let RecvAncillaryMessage::ScmRights(it) = m {
                        got.extend(it);
                    }
                }
                Ok(r.bytes)
            })
            .await?;
        for fd in got {
            // macOS has no MSG_CMSG_CLOEXEC; set it at once so nothing this process starts inherits the client.
            rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)?;
            self.fds.push_back(fd);
        }
        Ok(n)
    }

    pub(super) fn import(&mut self, _trailer: &[u8]) -> io::Result<OwnedFd> {
        self.fds.pop_front().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "a frame announced a handle that did not arrive",
            )
        })
    }
}

impl Writer {
    pub(super) fn export(&self, h: OwnedFd) -> io::Result<(Vec<u8>, Option<OwnedFd>)> {
        Ok((Vec::new(), Some(h)))
    }

    pub(super) async fn write(
        &mut self,
        bytes: &[u8],
        mut ride: Option<OwnedFd>,
    ) -> io::Result<()> {
        let mut off = 0;
        while let Some(rest) = bytes.get(off..).filter(|r| !r.is_empty()) {
            let s = &self.s;
            let n = s
                .async_io(Interest::WRITABLE, || {
                    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
                    let mut anc = SendAncillaryBuffer::new(&mut space);
                    let one;
                    if let Some(f) = ride.as_ref() {
                        one = [f.as_fd()];
                        if !anc.push(SendAncillaryMessage::ScmRights(&one)) {
                            return Err(io::Error::other("ancillary buffer too small"));
                        }
                    }
                    rustix::net::sendmsg(&**s, &[IoSlice::new(rest)], &mut anc, send_flags())
                        .map_err(io::Error::from)
                })
                .await?;
            // The handle travelled with the first byte the kernel accepted; our copy closes here.
            ride = None;
            off += n;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn recv_flags() -> RecvFlags {
    RecvFlags::CMSG_CLOEXEC
}
#[cfg(not(target_os = "linux"))]
fn recv_flags() -> RecvFlags {
    RecvFlags::empty()
}
#[cfg(target_os = "linux")]
fn send_flags() -> SendFlags {
    SendFlags::NOSIGNAL
}
#[cfg(not(target_os = "linux"))]
fn send_flags() -> SendFlags {
    SendFlags::empty()
}
