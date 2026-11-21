//! Loopback ports for the processes the engine starts.

use std::io;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU32, Ordering};

/// Where ports come from: below every system's ephemeral range (Linux starts at 32768, macOS and Windows at 49152).
/// A port from `bind(0)` is one the system hands out by itself, so between the probe and the service's own bind an
/// outgoing connection, or another program probing the same way, could be given it: "Address already in use" for a
/// service that had done nothing wrong (doze-engine's catalogue test on macOS, PR #111; os error 10048 on Windows,
/// CI run 44499950273). Nothing is handed a port from this range unless it asks for that port by number.
const PORTS: std::ops::Range<u32> = 20_000..32_000;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A loopback port nothing listens on now (Go's `netutil.FreePort`), from below the ephemeral range, and not one this
/// process handed out until it has gone through the whole range (12,000 ports). Each process starts at its own
/// offset, so two daemons starting at once rarely probe the same ports.
pub(crate) fn free_port() -> io::Result<u16> {
    let span = PORTS.end - PORTS.start;
    let start = std::process::id().wrapping_mul(97) % span;
    for _ in 0..span {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let Ok(port) = u16::try_from(PORTS.start + (start.wrapping_add(n)) % span) else {
            continue;
        };
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "no free loopback port between 20000 and 32000",
    ))
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
}
