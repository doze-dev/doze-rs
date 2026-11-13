//! A port another process holds (the PortClash board, Go `serve/portclash.go`): the port a crash says was taken,
//! and what holds it, from `lsof` (or `ss` where there is none, on Linux). A lookup is a fork and tens of
//! milliseconds, so each port's answer is kept for a few seconds, and a page asks off the UI thread.

use std::collections::HashMap;
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// The process listening on a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub port: u16,
    pub pid: i64,
    pub command: String,
    /// It is a doze program: the page words it differently, since stopping it is not the way out.
    pub doze: bool,
    /// Which stack's service it is, when it is one.
    pub stack: String,
    pub service: String,
}

// fuzz:parser fuzz_port
/// The port a bind failure names, in the words Go, Node, Python and Ruby use ("address already in use", `EADDRINUSE`,
/// `Errno 48`/`98`), the last `:port` in the text that says so. (Go `portInUse`.)
pub fn port_in_use<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<u16> {
    texts.into_iter().find_map(|t| {
        let lower = t.to_ascii_lowercase();
        let in_use = [
            "address already in use",
            "eaddrinuse",
            "errno 48",
            "errno 98",
        ]
        .iter()
        .any(|w| lower.contains(w));
        if !in_use {
            return None;
        }
        last_port(t)
    })
}

/// The last `:NN…` (two to five digits, then not a word character) in `t`.
fn last_port(t: &str) -> Option<u16> {
    let b = t.as_bytes();
    let mut found = None;
    for (i, _) in t.match_indices(':') {
        let digits = b
            .get(i + 1..)
            .unwrap_or_default()
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        let after = b.get(i + 1 + digits).copied();
        let word = after.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_');
        if (2..=5).contains(&digits) && !word {
            let p = t
                .get(i + 1..i + 1 + digits)
                .and_then(|s| s.parse::<u32>().ok());
            if let Some(p) = p.filter(|p| (1..65_536).contains(p)) {
                found = u16::try_from(p).ok();
            }
        }
    }
    found
}

/// How long an answer is kept.
const KEPT: Duration = Duration::from_secs(3);

type Seen = HashMap<u16, (Instant, Option<Holder>)>;

static SEEN: LazyLock<Mutex<Seen>> = LazyLock::new(Mutex::default);

/// What listens on `port`, looked up at most every few seconds; `None` when nothing is found.
pub fn holder(port: u16) -> Option<Holder> {
    if let Ok(seen) = SEEN.lock()
        && let Some((at, h)) = seen.get(&port)
        && at.elapsed() < KEPT
    {
        return h.clone();
    }
    let h = lsof(port).or_else(|| ss(port));
    if let Ok(mut seen) = SEEN.lock() {
        seen.insert(port, (Instant::now(), h.clone()));
    }
    h
}

/// Forgets a port's holder: doze just stopped it.
pub fn forget(port: u16) {
    if let Ok(mut seen) = SEEN.lock() {
        seen.remove(&port);
    }
}

fn lsof(port: u16) -> Option<Holder> {
    let out = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fpc"])
        .output()
        .ok()?;
    parse_lsof(port, &String::from_utf8_lossy(&out.stdout))
}

// fuzz:parser fuzz_port
/// `lsof -Fpc`: `p<pid>` and `c<command>` lines; the first of each.
pub fn parse_lsof(port: u16, out: &str) -> Option<Holder> {
    let (mut pid, mut command) = (0i64, String::new());
    for line in out.lines() {
        if let Some(p) = line.strip_prefix('p').filter(|_| pid == 0) {
            pid = p.trim().parse().unwrap_or(0);
        } else if let Some(c) = line.strip_prefix('c').filter(|_| command.is_empty()) {
            c.clone_into(&mut command);
        }
    }
    (pid > 0).then(|| held(port, pid, command))
}

fn ss(port: u16) -> Option<Holder> {
    let out = Command::new("ss")
        .args(["-ltnpH", &format!("sport = :{port}")])
        .output()
        .ok()?;
    parse_ss(port, &String::from_utf8_lossy(&out.stdout))
}

// fuzz:parser fuzz_port
/// `ss -ltnp`: `… users:(("node",pid=7731,fd=20))`.
pub fn parse_ss(port: u16, out: &str) -> Option<Holder> {
    let (_, rest) = out.split_once("((\"")?;
    let (command, rest) = rest.split_once('"')?;
    let (_, rest) = rest.split_once("pid=")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let pid = digits.parse().ok().filter(|&p: &i64| p > 0)?;
    Some(held(port, pid, command.to_owned()))
}

fn held(port: u16, pid: i64, command: String) -> Holder {
    Holder {
        port,
        pid,
        doze: command == "doze" || command.starts_with("doze-"),
        command,
        stack: String::new(),
        service: String::new(),
    }
}

/// Marks a holder that is one of doze's own: a service of a stack the hub knows (its pid, or its parent's, is that
/// service's), so the page says which and does not offer to stop it. `stacks` are (stack, service, pid).
pub fn owned_by<'a>(h: &mut Holder, stacks: impl IntoIterator<Item = (&'a str, &'a str, i64)>) {
    let parent = Command::new("ps")
        .args(["-o", "ppid=", "-p", &h.pid.to_string()])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<i64>()
                .ok()
        })
        .filter(|&p| p > 1);
    for (stack, service, pid) in stacks {
        if pid > 0 && (pid == h.pid || Some(pid) == parent) {
            h.doze = true;
            stack.clone_into(&mut h.stack);
            service.clone_into(&mut h.service);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_lsof, parse_ss, port_in_use};

    #[test]
    fn the_port_a_bind_failure_names() {
        let go = "listen tcp 127.0.0.1:8000: bind: address already in use";
        assert_eq!(port_in_use([go]), Some(8000));
        assert_eq!(
            port_in_use(["Error: listen EADDRINUSE: address already in use :::3000"]),
            Some(3000)
        );
        assert_eq!(
            port_in_use(["OSError: [Errno 48] Address already in use", "x :5000 y"]),
            None
        );
        assert_eq!(
            port_in_use(["panic: STRIPE_KEY is required", "cmd/api/config.go:41"]),
            None
        );
        assert_eq!(port_in_use(["address already in use :99999"]), None);
        assert_eq!(port_in_use(["address already in use at 10:30am"]), None);
    }

    #[test]
    fn holders_from_lsof_and_ss() {
        let h = parse_lsof(7799, "p4242\ncnode\nf20\np9\ncother\n").unwrap();
        assert_eq!((h.pid, h.command.as_str(), h.doze), (4242, "node", false));
        assert!(parse_lsof(7799, "").is_none());
        let s = parse_ss(
            3000,
            "LISTEN 0 511 *:3000 *:* users:((\"doze-kafka\",pid=7731,fd=20))",
        )
        .unwrap();
        assert_eq!(
            (s.pid, s.command.as_str(), s.doze),
            (7731, "doze-kafka", true)
        );
        assert!(parse_ss(1, "users:((\"x\",pid=0,fd=1))").is_none());
    }
}
