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

/// A loopback port nobody is using right now.
pub fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}
