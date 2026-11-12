//! Every OS difference the module transport needs, in one place (the shape of
//! `doze-os`, PLAN.md D11). Unix: socketpair as the module's stdin, SCM_RIGHTS.
//! Windows: a named pipe with an unguessable name (client pid verified), a Job
//! object that kills the module tree when the host goes, WSADuplicateSocketW.

pub use imp::*;

#[cfg(unix)]
mod imp {
    use anyhow::{Context, Result};
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags};
    use std::collections::VecDeque;
    use std::io::{IoSlice, IoSliceMut};
    use std::mem::MaybeUninit;
    use std::os::fd::{AsFd, OwnedFd};
    use std::process::Stdio;
    use std::sync::Arc;
    use tokio::io::Interest;
    use tokio::net::UnixStream;

    /// A handle that can be passed to the other side (a client connection).
    pub type Passable = OwnedFd;

    pub fn passable_from_tcp(s: std::net::TcpStream) -> Passable {
        OwnedFd::from(s)
    }
    pub fn tcp_from_passable(p: Passable) -> std::net::TcpStream {
        std::net::TcpStream::from(p)
    }

    pub struct Conn(UnixStream);

    pub fn init() -> Result<()> {
        Ok(())
    }

    /// Host side: start `cmd` as a module with the control socket as its stdin.
    pub async fn spawn_module(mut cmd: tokio::process::Command) -> Result<(tokio::process::Child, Conn, Job)> {
        let (ours, theirs) = std::os::unix::net::UnixStream::pair()?;
        cmd.stdin(Stdio::from(OwnedFd::from(theirs)));
        let child = cmd.spawn()?;
        ours.set_nonblocking(true)?;
        Ok((child, Conn(UnixStream::from_std(ours)?), Job))
    }

    /// Module side: take the control socket from stdin, then point fd 0 at /dev/null.
    pub fn module_conn() -> Result<Conn> {
        let ctl = std::io::stdin().as_fd().try_clone_to_owned()?;
        let null = rustix::fs::open("/dev/null", rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC, rustix::fs::Mode::empty())?;
        rustix::stdio::dup2_stdin(&null)?;
        let s = std::os::unix::net::UnixStream::from(ctl);
        s.set_nonblocking(true)?;
        Ok(Conn(UnixStream::from_std(s)?))
    }

    /// Unix has no kill-on-close job; process groups and EOF on the control socket do that work.
    pub struct Job;
    impl Job {
        pub fn new() -> Result<Job> {
            Ok(Job)
        }
        pub fn assign(&self, _child: &tokio::process::Child) -> Result<()> {
            Ok(())
        }
    }

    pub fn split(c: Conn) -> (Reader, Writer) {
        let s = Arc::new(c.0);
        (Reader { s: s.clone(), fds: VecDeque::new() }, Writer { s })
    }

    pub struct Reader {
        s: Arc<UnixStream>,
        fds: VecDeque<OwnedFd>,
    }
    pub struct Writer {
        s: Arc<UnixStream>,
    }

    /// Bytes appended to a frame body that carries a handle (none on Unix: the fd rides as ancillary data).
    pub const TRAILER: usize = 0;

    impl Reader {
        pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            let mut got: Vec<OwnedFd> = Vec::new();
            let s = &self.s;
            let n = s
                .async_io(Interest::READABLE, || {
                    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
                    let mut anc = RecvAncillaryBuffer::new(&mut space);
                    let r = rustix::net::recvmsg(&**s, &mut [IoSliceMut::new(buf)], &mut anc, recv_flags())?;
                    for m in anc.drain() {
                        if let RecvAncillaryMessage::ScmRights(it) = m {
                            got.extend(it);
                        }
                    }
                    Ok(r.bytes)
                })
                .await?;
            for fd in got {
                rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)?;
                self.fds.push_back(fd);
            }
            Ok(n)
        }
        pub fn import(&mut self, _trailer: &[u8]) -> Result<Passable> {
            self.fds.pop_front().context("frame announced a handle that did not arrive")
        }
    }

    impl Writer {
        /// Returns the trailer for a frame carrying `p`, and what must ride with the bytes.
        pub fn export(&self, p: Passable) -> Result<(Vec<u8>, Option<Passable>)> {
            Ok((Vec::new(), Some(p)))
        }
        pub async fn write(&mut self, bytes: &[u8], mut pass: Option<Passable>) -> Result<()> {
            let mut off = 0;
            while off < bytes.len() {
                let s = &self.s;
                let n = s
                    .async_io(Interest::WRITABLE, || {
                        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
                        let mut anc = SendAncillaryBuffer::new(&mut space);
                        let b;
                        if let Some(f) = pass.as_ref() {
                            b = [f.as_fd()];
                            anc.push(SendAncillaryMessage::ScmRights(&b));
                        }
                        rustix::net::sendmsg(&**s, &[IoSlice::new(&bytes[off..])], &mut anc, send_flags()).map_err(Into::into)
                    })
                    .await?;
                pass = None;
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

    pub fn alive(pid: u32) -> bool {
        rustix::process::Pid::from_raw(pid as i32).is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
    }

    pub fn rss_kb(pid: u32) -> u64 {
        std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
            .unwrap_or(0)
    }

    pub fn new_group(_cmd: &mut tokio::process::Command) {}

    pub fn send_break(pid: u32) -> Result<()> {
        let p = rustix::process::Pid::from_raw(pid as i32).context("pid")?;
        rustix::process::kill_process(p, rustix::process::Signal::TERM)?;
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use anyhow::{Context, Result, bail};
    use std::os::windows::io::{AsRawHandle, AsRawSocket, FromRawSocket, OwnedSocket};
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE};
    use windows_sys::Win32::Networking::WinSock::{
        FROM_PROTOCOL_INFO, INVALID_SOCKET, SOCKET, WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSADATA, WSADuplicateSocketW,
        WSAGetLastError, WSAPROTOCOL_INFOW, WSASocketW, WSAStartup,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    pub type Passable = OwnedSocket;

    pub fn passable_from_tcp(s: std::net::TcpStream) -> Passable {
        OwnedSocket::from(s)
    }
    pub fn tcp_from_passable(p: Passable) -> std::net::TcpStream {
        std::net::TcpStream::from(p)
    }

    pub enum Conn {
        Server(NamedPipeServer),
        Client(NamedPipeClient),
    }

    impl tokio::io::AsyncRead for Conn {
        fn poll_read(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<std::io::Result<()>> {
            match self.get_mut() {
                Conn::Server(p) => std::pin::Pin::new(p).poll_read(cx, buf),
                Conn::Client(p) => std::pin::Pin::new(p).poll_read(cx, buf),
            }
        }
    }
    impl tokio::io::AsyncWrite for Conn {
        fn poll_write(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &[u8]) -> std::task::Poll<std::io::Result<usize>> {
            match self.get_mut() {
                Conn::Server(p) => std::pin::Pin::new(p).poll_write(cx, buf),
                Conn::Client(p) => std::pin::Pin::new(p).poll_write(cx, buf),
            }
        }
        fn poll_flush(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
            match self.get_mut() {
                Conn::Server(p) => std::pin::Pin::new(p).poll_flush(cx),
                Conn::Client(p) => std::pin::Pin::new(p).poll_flush(cx),
            }
        }
        fn poll_shutdown(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
            match self.get_mut() {
                Conn::Server(p) => std::pin::Pin::new(p).poll_shutdown(cx),
                Conn::Client(p) => std::pin::Pin::new(p).poll_shutdown(cx),
            }
        }
    }

    pub fn init() -> Result<()> {
        let mut data: WSADATA = unsafe { std::mem::zeroed() };
        // SAFETY: WSAStartup writes into `data`, which we own; 0x0202 is Winsock 2.2.
        let r = unsafe { WSAStartup(0x0202, &mut data) };
        if r != 0 {
            bail!("WSAStartup failed: {r}");
        }
        Ok(())
    }

    fn pipe_name() -> String {
        use std::hash::{BuildHasher, Hasher};
        let mut a = std::collections::hash_map::RandomState::new().build_hasher();
        a.write_u32(std::process::id());
        let mut b = std::collections::hash_map::RandomState::new().build_hasher();
        b.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        format!(r"\\.\pipe\doze-module-{:016x}{:016x}", a.finish(), b.finish())
    }

    /// Host side: a fresh, single-instance pipe; the module connects to it by name, and
    /// the host checks that the client is the process it just started.
    pub async fn spawn_module(mut cmd: tokio::process::Command) -> Result<(tokio::process::Child, Conn, Job)> {
        let name = pipe_name();
        let server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name)?;
        cmd.env("DOZE_MODULE_PIPE", &name).stdin(Stdio::null());
        let child = cmd.spawn()?;
        let job = Job::new()?;
        job.assign(&child)?;
        tokio::time::timeout(Duration::from_secs(10), server.connect()).await.context("module did not connect in 10 s")??;
        let mut client_pid: u32 = 0;
        // SAFETY: the handle is the live pipe owned by `server`; the out-pointer is ours.
        let ok = unsafe { GetNamedPipeClientProcessId(server.as_raw_handle() as HANDLE, &mut client_pid) };
        let want = child.id().unwrap_or(0);
        if ok == 0 || client_pid != want {
            bail!("pipe client is pid {client_pid}, expected the module {want}");
        }
        Ok((child, Conn::Server(server), job))
    }

    pub fn module_conn() -> Result<Conn> {
        let name = std::env::var("DOZE_MODULE_PIPE").context("DOZE_MODULE_PIPE not set")?;
        Ok(Conn::Client(ClientOptions::new().open(&name)?))
    }

    /// A kill-on-close Job object: when the host's handle closes (the host exits or is
    /// killed), Windows terminates every process in the job.
    pub struct Job(HANDLE);
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new() -> Result<Job> {
            // SAFETY: null attributes and name create an anonymous job; the handle is checked.
            let h = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if h.is_null() {
                bail!("CreateJobObjectW failed: {}", std::io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `info` is the struct this information class expects, with its exact size.
            let ok = unsafe {
                SetInformationJobObject(h, JobObjectExtendedLimitInformation, (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), std::mem::size_of_val(&info) as u32)
            };
            if ok == 0 {
                bail!("SetInformationJobObject failed: {}", std::io::Error::last_os_error());
            }
            Ok(Job(h))
        }
        pub fn assign(&self, child: &tokio::process::Child) -> Result<()> {
            let ph = child.raw_handle().context("child already reaped")?;
            // SAFETY: both handles are live: ours, and the child's process handle tokio holds.
            let ok = unsafe { AssignProcessToJobObject(self.0, ph as HANDLE) };
            if ok == 0 {
                bail!("AssignProcessToJobObject failed: {}", std::io::Error::last_os_error());
            }
            Ok(())
        }
    }
    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: we own the handle and close it once.
            unsafe { CloseHandle(self.0) };
        }
    }

    pub fn split(c: Conn) -> (Reader, Writer) {
        let (r, w) = tokio::io::split(c);
        (Reader { r }, Writer { w, peer_pid: 0 })
    }

    pub struct Reader {
        r: ReadHalf<Conn>,
    }
    pub struct Writer {
        w: WriteHalf<Conn>,
        pub peer_pid: u32,
    }

    pub const TRAILER: usize = std::mem::size_of::<WSAPROTOCOL_INFOW>();

    impl Reader {
        pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            match self.r.read(buf).await {
                Ok(n) => Ok(n),
                // The host went away: a broken pipe is the same as EOF.
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(0),
                Err(e) => Err(e.into()),
            }
        }
        pub fn import(&mut self, trailer: &[u8]) -> Result<Passable> {
            if trailer.len() != TRAILER {
                bail!("handle trailer is {} bytes, expected {TRAILER}", trailer.len());
            }
            // SAFETY: WSAPROTOCOL_INFOW is plain data; the length was checked; read_unaligned copies it out.
            let info: WSAPROTOCOL_INFOW = unsafe { std::ptr::read_unaligned(trailer.as_ptr().cast()) };
            // SAFETY: `info` came from WSADuplicateSocketW for this process; WSASocketW returns a new socket or INVALID_SOCKET.
            let s: SOCKET = unsafe { WSASocketW(FROM_PROTOCOL_INFO, FROM_PROTOCOL_INFO, FROM_PROTOCOL_INFO, &info, 0, WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT) };
            if s == INVALID_SOCKET {
                // SAFETY: plain call.
                bail!("WSASocketW(FROM_PROTOCOL_INFO) failed: {}", unsafe { WSAGetLastError() });
            }
            // SAFETY: `s` is a fresh socket this process now owns.
            Ok(unsafe { OwnedSocket::from_raw_socket(s as u64) })
        }
    }

    impl Writer {
        pub fn export(&self, p: Passable) -> Result<(Vec<u8>, Option<Passable>)> {
            if self.peer_pid == 0 {
                bail!("no peer pid to duplicate a socket into");
            }
            let mut info: WSAPROTOCOL_INFOW = unsafe { std::mem::zeroed() };
            // SAFETY: the socket is live (we own `p`); the out-pointer is ours.
            let r = unsafe { WSADuplicateSocketW(p.as_raw_socket() as SOCKET, self.peer_pid, &mut info) };
            if r != 0 {
                // SAFETY: plain call.
                bail!("WSADuplicateSocketW failed: {}", unsafe { WSAGetLastError() });
            }
            // SAFETY: plain-data struct viewed as bytes for its exact size.
            let bytes = unsafe { std::slice::from_raw_parts((&info as *const WSAPROTOCOL_INFOW).cast::<u8>(), TRAILER) }.to_vec();
            // Our copy closes when `p` drops here; the module holds its own.
            drop(p);
            Ok((bytes, None))
        }
        pub async fn write(&mut self, bytes: &[u8], _pass: Option<Passable>) -> Result<()> {
            self.w.write_all(bytes).await?;
            Ok(())
        }
    }

    fn open(pid: u32) -> Option<HANDLE> {
        // SAFETY: plain call; a null return is handled.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if h.is_null() { None } else { Some(h) }
    }

    pub fn alive(pid: u32) -> bool {
        let Some(h) = open(pid) else { return false };
        let mut code: u32 = 0;
        // SAFETY: `h` is a live process handle we opened and close below.
        let ok = unsafe { GetExitCodeProcess(h, &mut code) };
        unsafe { CloseHandle(h) };
        ok != 0 && code == STILL_ACTIVE as u32
    }

    pub fn rss_kb(pid: u32) -> u64 {
        let Some(h) = open(pid) else { return 0 };
        let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is the counters struct with its size; `h` is live and closed below.
        let ok = unsafe { K32GetProcessMemoryInfo(h, &mut c, std::mem::size_of_val(&c) as u32) };
        unsafe { CloseHandle(h) };
        if ok == 0 { 0 } else { (c.WorkingSetSize / 1024) as u64 }
    }

    pub fn new_group(cmd: &mut tokio::process::Command) {
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    pub fn send_break(pid: u32) -> Result<()> {
        use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
        // SAFETY: plain call; the group id is the pid of a process started with CREATE_NEW_PROCESS_GROUP.
        let ok = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) };
        if ok == 0 {
            bail!("GenerateConsoleCtrlEvent failed: {}", std::io::Error::last_os_error());
        }
        Ok(())
    }
}
