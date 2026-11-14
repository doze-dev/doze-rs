//! `instance.json` in the data directory: where a running doze-aws is and what it serves, written once it listens and removed when
//! it stops cleanly. The data directory's lock says whether the process that wrote it is still running (a file left by a crash,
//! or a reused pid, is not mistaken for a live server).

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file's name.
pub(crate) const FILE: &str = "instance.json";

/// A running doze-aws, as it describes itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Instance {
    /// The process.
    pub(crate) pid: u32,
    /// The URL to reach it at (not in the health answer: the asker knows it).
    #[serde(default)]
    pub(crate) endpoint: String,
    /// The region `env` points at.
    pub(crate) region: String,
    /// The account.
    pub(crate) account: String,
    /// `trust` or `verify`.
    pub(crate) auth: String,
    /// The key `--auth verify` accepts (only under verify).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) access_key: Option<String>,
    /// Its secret (only under verify; the file is readable by its owner alone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) secret_key: Option<String>,
    /// The services it serves.
    pub(crate) services: Vec<String>,
    /// Its version.
    pub(crate) version: String,
    /// When it started, in seconds since 1970.
    pub(crate) started: u64,
    /// Its data directory.
    pub(crate) data_dir: String,
}

impl Instance {
    /// What `GET /_doze/health` answers: everything but the keys.
    pub(crate) fn health(&self) -> serde_json::Map<String, serde_json::Value> {
        let public = Self {
            access_key: None,
            secret_key: None,
            ..self.clone()
        };
        match serde_json::to_value(public) {
            Ok(serde_json::Value::Object(mut m)) => {
                m.remove("endpoint");
                m.insert("name".into(), "doze-aws".into());
                m
            }
            _ => serde_json::Map::new(),
        }
    }
}

/// The URL a client on this machine reaches `addr` at: a wildcard address is reached on loopback.
pub(crate) fn endpoint_of(addr: SocketAddr) -> String {
    let ip = match addr.ip() {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    format!("http://{}", SocketAddr::new(ip, addr.port()))
}

/// Writes the file (whole: another process never reads half of it).
pub(crate) fn write(dir: &Path, instance: &Instance) -> io::Result<()> {
    let tmp = dir.join(format!("{FILE}.tmp"));
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    let body = serde_json::to_vec_pretty(instance).map_err(io::Error::other)?;
    file.write_all(&body)?;
    file.sync_all()?;
    std::fs::rename(&tmp, dir.join(FILE))
}

/// Removes the file.
pub(crate) fn remove(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(FILE));
}

/// The file's contents, if it is there and readable.
pub(crate) fn read(dir: &Path) -> Option<Instance> {
    let bytes = std::fs::read(dir.join(FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Whether a process holds the data directory's lock.
pub(crate) fn held(dir: &Path) -> bool {
    let Ok(file) = File::open(dir.join("lock")) else {
        return false;
    };
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(_)) => false,
    }
}

/// The doze-aws running on `dir`, if one is.
pub(crate) fn running(dir: &Path) -> Option<Instance> {
    held(dir).then(|| read(dir)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_read_back_and_live_only_while_the_lock_is_held() {
        let dir = std::env::temp_dir().join(format!("doze-aws-instance-{}", std::process::id()));
        let lock = doze_runtime::DataDirLock::take(&dir, "doze-aws").unwrap();
        let me = Instance {
            pid: 7,
            endpoint: "http://127.0.0.1:4566".into(),
            secret_key: Some("s".into()),
            ..Instance::default()
        };
        write(&dir, &me).unwrap();
        assert_eq!(running(&dir), Some(me.clone()));
        assert!(!me.health().contains_key("secret_key"));
        drop(lock);
        assert_eq!(running(&dir), None, "a file without a holder is stale");
        remove(&dir);
        assert_eq!(read(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_wildcard_address_is_reached_on_loopback() {
        assert_eq!(
            endpoint_of("0.0.0.0:1".parse().unwrap()),
            "http://127.0.0.1:1"
        );
        assert_eq!(endpoint_of("[::]:1".parse().unwrap()), "http://[::1]:1");
        assert_eq!(
            endpoint_of("10.0.0.2:1".parse().unwrap()),
            "http://10.0.0.2:1"
        );
    }
}
