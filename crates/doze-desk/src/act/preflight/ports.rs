//! Who listens where, for the checks before Up (Go `listening`, `bindable`): one `lsof` for every TCP port something
//! listens on (`ss` on Linux), so the checks cost one call; and whether this user could listen on a port now, which
//! catches what `lsof` cannot see (another user's process, or root's).

use std::collections::HashMap;
use std::net::TcpListener;
use std::process::Command;

use crate::ports::Holder;

/// Every TCP port something listens on, with who holds it.
pub fn listening() -> HashMap<u16, Holder> {
    let lsof = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpcn"])
        .output();
    if let Ok(o) = &lsof
        && (o.status.success() || !o.stdout.is_empty())
    {
        return parse_lsof_all(&String::from_utf8_lossy(&o.stdout));
    }
    if cfg!(target_os = "macos") {
        return HashMap::new();
    }
    Command::new("ss")
        .args(["-ltnpH"])
        .output()
        .map(|o| parse_ss_all(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Whether this user could listen on `port` on loopback now.
pub fn bindable(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn holder(port: u16, pid: i64, command: &str) -> Holder {
    Holder {
        port,
        pid,
        doze: command == "doze" || command.starts_with("doze-"),
        command: command.to_owned(),
        stack: String::new(),
        service: String::new(),
    }
}

/// The port after the last `:` of an address (`*:8080`, `127.0.0.1:5432`, `[::1]:3000`).
fn port_of(addr: &str) -> Option<u16> {
    let (_, p) = addr.rsplit_once(':')?;
    p.trim().parse().ok().filter(|p| *p > 0)
}

// fuzz:parser fuzz_listening
/// `lsof -Fpcn`: a `p<pid>` line, a `c<command>` line, then an `n<address>` line for each socket; the first holder of a
/// port is kept.
pub fn parse_lsof_all(out: &str) -> HashMap<u16, Holder> {
    let mut held = HashMap::new();
    let (mut pid, mut command) = (0i64, String::new());
    for line in out.lines() {
        let mut chars = line.chars();
        let rest = chars.as_str().get(1..).unwrap_or_default();
        match chars.next() {
            Some('p') => pid = rest.trim().parse().unwrap_or(0),
            Some('c') => rest.clone_into(&mut command),
            Some('n') => {
                if let Some(port) = port_of(rest) {
                    held.entry(port)
                        .or_insert_with(|| holder(port, pid, &command));
                }
            }
            _ => {}
        }
    }
    held
}

// fuzz:parser fuzz_listening
/// `ss -ltnpH`: one socket a line, its local address the fourth field, its process as `users:(("node",pid=7731,…))`.
pub fn parse_ss_all(out: &str) -> HashMap<u16, Holder> {
    let mut held = HashMap::new();
    for line in out.lines() {
        let Some(port) = line.split_whitespace().nth(3).and_then(port_of) else {
            continue;
        };
        let h = crate::ports::parse_ss(port, line).unwrap_or_else(|| holder(port, 0, ""));
        held.insert(port, h);
    }
    held
}

#[cfg(test)]
mod tests {
    use super::{bindable, parse_lsof_all, parse_ss_all};

    #[test]
    fn every_listener_from_lsof() {
        let out = "p4242\ncnode\nn*:3000\nn[::1]:3000\np88\ncdoze-kafka\nn127.0.0.1:9092\np9\ncx\nnlocalhost:http\n";
        let held = parse_lsof_all(out);
        assert_eq!(held.len(), 2);
        let node = held.get(&3000).map(|h| (h.pid, h.command.as_str(), h.doze));
        assert_eq!(node, Some((4242, "node", false)));
        assert!(held.get(&9092).is_some_and(|h| h.doze));
        assert!(parse_lsof_all("").is_empty());
        assert!(parse_lsof_all("p\nc\nn:\n").is_empty());
    }

    #[test]
    fn every_listener_from_ss() {
        let out = "LISTEN 0 511 *:3000 *:* users:((\"node\",pid=7731,fd=20))\nLISTEN 0 4096 127.0.0.1:5432 0.0.0.0:*\n";
        let held = parse_ss_all(out);
        assert_eq!(held.get(&3000).map(|h| h.pid), Some(7731));
        assert_eq!(held.get(&5432).map(|h| h.pid), Some(0));
    }

    #[test]
    fn a_port_held_here_is_not_bindable() {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(!bindable(port));
        drop(l);
    }
}
