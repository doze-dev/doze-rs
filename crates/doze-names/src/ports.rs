//! `names.ports.json`, beside the registry: the translated ports alone, host → the process that set them and
//! public → private.
//!
//! The registry's `Entry::ports` says the same, but every doze program on the machine rewrites the registry, and a Go
//! build from before that field read the file into a type without it and wrote it back, so the mappings vanished.
//! Nothing older knows this file, so nothing older can drop it. The network daemon reads both (Go's netd from version
//! 3, and ours), so Rust programs write both, in Go's format.
//!
//! Go also ran a `heal` thread in every program, putting its ports back into the registry within 250 ms of an older
//! build stripping them. Rust keeps fields it does not know when it rewrites the registry, and every daemon that
//! translates reads this file, so a heal only ever helped a daemon older than version 3, which setup replaces. It is
//! not ported (one thread fewer in every program); the reason is kept here.

use crate::Error;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The file's name, in the registry's folder.
pub const PORTS_FILE: &str = "names.ports.json";

/// One host's translated ports, and whose they are.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PortsEntry {
    #[serde(default)]
    pub pid: i64,
    /// The address the ports were set at: a name claimed again at another address does not carry them.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ip: String,
    #[serde(default)]
    pub ports: BTreeMap<String, u16>,
}

/// The ports file beside the registry at `registry`.
pub fn ports_path(registry: &Path) -> PathBuf {
    registry.with_file_name(PORTS_FILE)
}

/// The ports file, or nothing (missing or unreadable alike: the registry still has the same ports).
pub fn read_ports(registry: &Path) -> BTreeMap<String, PortsEntry> {
    std::fs::read(ports_path(registry))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

/// Sets `host`'s ports for `pid` (none: drops them, if they are `pid`'s), dropping hosts whose process is gone, under
/// the file's own lock and with an atomic rename.
pub(crate) fn write_ports(
    registry: &Path,
    host: &str,
    ip: &str,
    pid: u32,
    ports: &BTreeMap<String, u16>,
) -> Result<(), Error> {
    let path = ports_path(registry);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _lock = doze_os::fs::lock(&path.with_extension("json.lock"))?;
    let mut m = read_ports(registry);
    m.retain(|_, e| u32::try_from(e.pid).is_ok_and(doze_os::process::alive));
    let pid = i64::from(pid);
    if ports.is_empty() {
        if m.get(host).is_some_and(|e| e.pid == pid) {
            m.remove(host);
        }
    } else {
        let entry = PortsEntry {
            pid,
            ip: ip.to_string(),
            ports: ports.clone(),
        };
        m.insert(host.to_string(), entry);
    }
    let mut text = serde_json::to_string_pretty(&m).map_err(std::io::Error::other)?;
    text.push('\n');
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}
