//! Loopback ports for the processes the engine starts.
//!
//! A port is chosen by probing it (bind, then let go), and the process binds it a moment later. No choice can close
//! that gap for every program on the machine (another doze daemon starting its own services, any app that asks for a
//! port by number), so a start whose process died because the port it was given was taken meanwhile is started again
//! on another one (`lost`, used by `boot::start`).

use std::collections::BTreeSet;
use std::io;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

/// Where ports come from: below every system's ephemeral range (Linux starts at 32768, macOS and Windows at 49152).
/// A port from `bind(0)` is one the system hands out by itself, so between the probe and the service's own bind an
/// outgoing connection, or another program probing the same way, could be given it: "Address already in use" for a
/// service that had done nothing wrong (doze-engine's catalogue test on macOS, PR #111; os error 10048 on Windows,
/// CI run 44499950273). Nothing is handed a port from this range unless it asks for that port by number.
const PORTS: std::ops::Range<u32> = 20_000..32_000;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// Every port this process handed out: only those are worth choosing again when they turn out taken (a port a plan
/// fixed by number would be taken again).
static HANDED: Mutex<BTreeSet<u16>> = Mutex::new(BTreeSet::new());

/// A loopback port nothing listens on now (Go's `netutil.FreePort`), from below the ephemeral range, and not one this
/// process handed out until it has gone through the whole range (12,000 ports). Each process starts at its own
/// offset, so two daemons starting at once rarely probe the same ports.
///
/// Public for planners and tests that give a process a port of their own choosing: a port from `bind(0)` is one the
/// system hands out again, to the next `bind(0)` in the same process (two test helpers given one port, macOS) or to an
/// outgoing connection.
pub fn free_port() -> io::Result<u16> {
    let span = PORTS.end - PORTS.start;
    let start = std::process::id().wrapping_mul(97) % span;
    for _ in 0..span {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let Ok(port) = u16::try_from(PORTS.start + (start.wrapping_add(n)) % span) else {
            continue;
        };
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            HANDED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(port);
            return Ok(port);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "no free loopback port between 20000 and 32000",
    ))
}

/// Whether `port`, one this process handed out, is held by something else now: asked once the process given it is
/// gone (so whatever holds it is not that process), it means the port was taken between the probe and the process's
/// own bind, and another port will do.
pub(crate) fn lost(port: u16) -> bool {
    let ours = HANDED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(&port);
    ours && TcpListener::bind(("127.0.0.1", port))
        .is_err_and(|e| e.kind() == io::ErrorKind::AddrInUse)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ports asked for one after another, each released before the next is asked for, are all different.
    #[test]
    fn a_port_is_not_handed_out_twice_in_a_row() {
        let ports: Vec<u16> = (0..64).map(|_| free_port().unwrap()).collect();
        let mut sorted = ports.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ports.len(), "{ports:?}");
    }

    /// A port handed out is never one the system would hand out by itself.
    #[test]
    fn a_port_is_below_the_ephemeral_range() {
        for _ in 0..64 {
            let p = free_port().unwrap();
            assert!((20_000..32_000).contains(&p), "{p}");
        }
    }

    /// A port handed out and then taken by someone else is lost; free again, or never handed out, it is not.
    ///
    /// The other test processes of a run draw ports from the same range, so one may bind this port while it is free;
    /// a port taken that way is tried again with another (the gates failed twice on it, 2026-11-23).
    #[test]
    fn a_handed_out_port_taken_by_someone_else_is_lost() {
        let mut seen = false;
        for _ in 0..20 {
            let p = free_port().unwrap();
            if lost(p) {
                continue;
            }
            let Ok(holder) = TcpListener::bind(("127.0.0.1", p)) else {
                continue;
            };
            assert!(lost(p), "held here, so held by someone");
            drop(holder);
            if lost(p) {
                continue;
            }
            seen = true;
            break;
        }
        assert!(
            seen,
            "twenty ports were each taken by another process meanwhile"
        );
        let other = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixed = other.local_addr().unwrap().port();
        assert!(!lost(fixed), "a port this process never handed out");
    }
}
