//! A throwaway doze home for one test: data and socket directories, logs, free ports. Never `~/.doze` (the plugin
//! spike once downloaded engines into the real one). Removed when dropped.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A private, temporary doze home.
#[derive(Debug)]
pub struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    /// A fresh sandbox. On Unix it lives under /tmp, not the long macOS temp path: a Unix socket's path must fit in
    /// about 100 bytes.
    pub fn new() -> std::io::Result<Sandbox> {
        let base = if cfg!(unix) {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = base.join(format!("dzt-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        doze_os::fs::private_dir(&root)?;
        Ok(Sandbox { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where an instance keeps its data.
    pub fn data_dir(&self, instance: &str) -> PathBuf {
        self.root.join("data").join(instance)
    }

    /// Where an instance's sockets go (short on purpose).
    pub fn socket_dir(&self, instance: &str) -> std::io::Result<PathBuf> {
        let d = self.root.join("s").join(instance);
        std::fs::create_dir_all(&d)?;
        Ok(d)
    }

    /// Where process output is written.
    pub fn logs(&self) -> std::io::Result<PathBuf> {
        let d = self.root.join("logs");
        std::fs::create_dir_all(&d)?;
        Ok(d)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Where handed-out ports come from: below every system's ephemeral range (Linux starts at 32768, macOS and
/// Windows at 49152), so an outgoing connection made in the gap between this probe and the engine's bind cannot be
/// given the same port. A port from the ephemeral range (`bind(":0")`) could be, and was: "events exited before it
/// was ready: Address already in use" on Linux CI. Not 20000..32000: the engine hands those out from the same
/// per-process offset, so a test and an engine in one process would be given the same port.
const PORTS: std::ops::Range<u32> = 10_000..20_000;

static NEXT_PORT: AtomicU32 = AtomicU32::new(0);

/// A loopback port nobody is using right now, outside the ephemeral range, and never the same one twice in this
/// process. Each process starts at its own offset so concurrent test binaries rarely ask for the same ports.
pub fn free_port() -> std::io::Result<u16> {
    let span = PORTS.end - PORTS.start;
    let start = std::process::id().wrapping_mul(97) % span;
    for _ in 0..span {
        let n = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
        let Ok(port) = u16::try_from(PORTS.start + (start + n) % span) else {
            continue;
        };
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        "no free loopback port below the ephemeral range",
    ))
}
