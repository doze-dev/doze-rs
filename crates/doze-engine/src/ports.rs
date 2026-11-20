//! Loopback ports for the processes the engine starts.

use std::collections::VecDeque;
use std::io;
use std::net::TcpListener;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a port handed out is not handed out again: long enough for the process given it to bind it.
const HOLD: Duration = Duration::from_secs(30);

/// How many ports are tried before giving up (each try is held open, so the system cannot offer it again).
const TRIES: usize = 32;

/// A loopback port nothing listens on now (Go's `netutil.FreePort`), and not one this process handed out in the last
/// 30 seconds. Windows' `bind(0)` offers a port again as soon as it is released, so two services started together
/// could be given one port, and the second failed with "address in use" (os error 10048, CI run 44499950273).
pub(crate) fn free_port() -> io::Result<u16> {
    static RECENT: Mutex<VecDeque<(u16, Instant)>> = Mutex::new(VecDeque::new());
    let mut recent = RECENT.lock().unwrap_or_else(PoisonError::into_inner);
    let now = Instant::now();
    while recent
        .front()
        .is_some_and(|(_, at)| now.duration_since(*at) > HOLD)
    {
        recent.pop_front();
    }
    let mut held = Vec::new();
    for _ in 0..TRIES {
        let l = TcpListener::bind(("127.0.0.1", 0))?;
        let port = l.local_addr()?.port();
        if !recent.iter().any(|(p, _)| *p == port) {
            recent.push_back((port, now));
            return Ok(port);
        }
        held.push(l);
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "every free loopback port offered was handed out in the last 30 seconds",
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
}
