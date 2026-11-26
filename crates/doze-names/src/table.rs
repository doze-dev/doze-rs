//! What the macOS daemon translates from: every live name's registered ports, by address, and the rule for how stale
//! its copy may be.

use crate::front::{INGRESS_BIND, INGRESS_KEY, INGRESS_TLS_BIND, INGRESS_TLS_KEY};
use crate::in_zone;
use crate::nat::PortTable;
use crate::pool::Net;
use crate::ports::{ports_path, read_ports};
use crate::registry::Entry;
use doze_os::fs::Watch;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// The ports the shared front door answers on, by the entry that records it and the bind that makes it reachable at
/// the daemon's address (the wildcard). Go answered port 80 only; https to a routed name was refused on macOS.
const FRONT_DOOR: [(&str, &str, u16); 2] = [
    (INGRESS_KEY, INGRESS_BIND, 80),
    (INGRESS_TLS_KEY, INGRESS_TLS_BIND, 443),
];

fn alive(e: &Entry) -> bool {
    u32::try_from(e.pid).is_ok_and(doze_os::process::alive)
}

/// The table, built from the registry at `path` as it stands: read without the lock and never rewritten (the file is
/// replaced by a rename, so a read is always of one whole version). A name whose process is gone is left out: its
/// private port may already be something else's.
pub fn read_port_table(path: &Path, net: Net) -> PortTable {
    let mut t = PortTable::default();
    let Some(m) = std::fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<BTreeMap<String, Entry>>(&raw).ok())
    else {
        return t;
    };
    let side = read_ports(path);
    let doors: Vec<u16> = FRONT_DOOR
        .iter()
        .filter(|(key, bind, _)| m.get(*key).is_some_and(|e| alive(e) && e.target == *bind))
        .map(|(_, _, port)| *port)
        .collect();
    for (host, e) in &m {
        let Some(ip) = e.addr().filter(|ip| net.in_pool(*ip)) else {
            continue;
        };
        if !in_zone(host) || !alive(e) {
            continue;
        }
        let mut ports = e.ports.clone();
        if let Some(s) = side.get(host)
            && s.pid == e.pid
            && (s.ip.is_empty() || s.ip == e.ip)
        {
            ports.extend(s.ports.iter().map(|(k, v)| (k.clone(), *v)));
        }
        let v = ip.octets();
        for (public, private) in &ports {
            if let Ok(public) = public.parse::<u16>()
                && public != 0
                && *private != 0
            {
                t.add(v, public, *private);
            }
        }
        // A routed name is answered by the front door, unless the service took the port for itself.
        if !e.target.is_empty() {
            for port in &doors {
                if !ports.contains_key(&port.to_string()) {
                    t.add(v, *port, *port);
                }
            }
        }
    }
    t
}

/// The daemon's copy of the table.
///
/// A connection to a service registered a moment ago, or whose port a restart just replaced, must be translated by
/// what the registry says now: a copy re-read only on a timer sends it to the last run's port, refused or dropped. So
/// the files are looked at again (one stat each) whenever a connection opens, and the table rebuilt when either was
/// written. Between connections it follows the files by an event ([`TableCache::follow`]), and a long timer remains for
/// what the files cannot say: a process that died without writing.
#[derive(Debug)]
pub struct TableCache {
    path: PathBuf,
    net: Net,
    stamp: Mutex<String>,
    table: RwLock<Arc<PortTable>>,
    /// Counts rebuilds, so the packet loop can keep its own copy and look at one atomic per packet rather than take
    /// the lock and the table's reference count.
    generation: AtomicU64,
}

impl TableCache {
    pub fn new(path: &Path, net: Net) -> TableCache {
        let c = TableCache {
            path: path.to_path_buf(),
            net,
            stamp: Mutex::new(String::new()),
            table: RwLock::new(Arc::default()),
            generation: AtomicU64::new(0),
        };
        c.refresh(true);
        c
    }

    /// The table to translate with.
    pub fn current(&self) -> Arc<PortTable> {
        self.table
            .read()
            .map_or_else(|e| e.into_inner().clone(), |t| t.clone())
    }

    /// Which rebuild [`TableCache::current`] is: it changes whenever the table does. Read it before the table.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Keeps the table in step with the files, on this thread, for good: rebuilt when something in `dir` (the
    /// registry's folder) changed and either file with it, and regardless every `every`. Both files are replaced by
    /// a rename, which is a change to the folder, so nothing is looked at while nothing is written: no wakeups while
    /// idle. Where the folder cannot be watched it looks every `poll` instead, as it always did.
    pub fn follow(&self, dir: &Path, every: Duration, poll: Duration) {
        let mut watch = match Watch::dir(dir) {
            Ok(w) if Watch::notifies() => Some(w),
            Ok(_) => None,
            Err(e) => {
                eprintln!(
                    "netd: cannot watch {} ({e}); looking every {poll:?}",
                    dir.display()
                );
                None
            }
        };
        loop {
            match watch.as_mut().map(|w| w.wait(every)) {
                Some(Ok(changed)) => self.refresh(!changed),
                Some(Err(e)) => {
                    eprintln!(
                        "netd: watching {} failed ({e}); looking every {poll:?}",
                        dir.display()
                    );
                    watch = None;
                }
                None => {
                    std::thread::sleep(poll);
                    self.refresh(true);
                }
            }
        }
    }

    /// Rebuilds the table if either file was written since it was read, or regardless when `force` is set.
    pub fn refresh(&self, force: bool) {
        let mut stamp = self
            .stamp
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Taken before the read: a write landing in between makes the next refresh read again, where a stamp taken
        // afterwards could describe a table older than it. The inode is in it because time and size are not enough:
        // two writes inside one tick of the file system's clock that leave the size the same (a restart swapping one
        // five-digit port for another) looked like no write at all.
        let now = self.stamp_now();
        if !force && now == *stamp {
            return;
        }
        let table = Arc::new(read_port_table(&self.path, self.net));
        match self.table.write() {
            Ok(mut t) => *t = table,
            Err(e) => *e.into_inner() = table,
        }
        self.generation.fetch_add(1, Ordering::Release);
        *stamp = now;
    }

    fn stamp_now(&self) -> String {
        let mut s = String::new();
        for p in [self.path.clone(), ports_path(&self.path)] {
            if let Ok(m) = std::fs::metadata(&p) {
                let nanos = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                let id = doze_os::fs::file_id(&p).unwrap_or(0);
                s.push_str(&format!("{id}/{nanos}/{};", m.len()));
            }
        }
        s
    }
}
