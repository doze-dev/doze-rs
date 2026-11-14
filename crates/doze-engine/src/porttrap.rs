//! The port trap (Go's `runtime/porttrap.go` and `portholder.ListeningPorts`): an app doze fronts that was never ready
//! most often ignored `$PORT` and listens somewhere else, or nowhere. When its check timed out while it still ran, the
//! error says so, naming the ports its process tree does listen on.
//!
//! Better than Go: when the system cannot say what listens (no `ps`, `lsof` or `ss`, as on Windows), there is no hint;
//! Go then said the app "listens on no port", which was a guess.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use tokio::process::Command;

/// How long asking the system may take (Go's).
const ASK: Duration = Duration::from_secs(3);

/// The `$PORT` doze gave a process, or 0 when it gave none.
pub fn env_port(env: &BTreeMap<String, String>) -> u16 {
    env.get("PORT").and_then(|p| p.parse().ok()).unwrap_or(0)
}

/// Why an app was never ready, when the reason is the trap: `""` when doze gave no `$PORT` or the app listens on it
/// (the check failed for another reason). `listening`: every TCP port its process tree listens on.
pub fn hint(name: &str, port: u16, listening: &[u16]) -> String {
    if port == 0 || listening.contains(&port) {
        return String::new();
    }
    if listening.is_empty() {
        return format!("{name} started but listens on no port; doze waited for $PORT ({port})");
    }
    let where_: Vec<String> = listening.iter().map(|p| format!(":{p}")).collect();
    format!(
        "{name} listens on {}, not on $PORT ({port}) that doze gave it; start it with --port $PORT (vite: --port $PORT \
         --strictPort), or set direct = true to have doze leave its port alone",
        where_.join(", ")
    )
}

/// The TCP ports `root`'s process tree listens on, in the order the system lists them; `None` when it cannot say.
pub async fn listening(root: u32) -> Option<Vec<u16>> {
    if root == 0 {
        return None;
    }
    let pids = match output("ps", &["-A", "-o", "pid=", "-o", "ppid="]).await {
        Some(ps) => descendants(root, &ps),
        None => vec![root],
    };
    if doze_os::which::which("lsof").is_some() {
        let list: Vec<String> = pids.iter().map(u32::to_string).collect();
        let list = list.join(",");
        let args = ["-nP", "-a", "-p", &list, "-iTCP", "-sTCP:LISTEN", "-Fn"];
        // lsof exits 1 when nothing matched, with nothing printed: that is "no port", not "cannot say".
        let out = output_any("lsof", &args).await?;
        return Some(lsof_ports(&out));
    }
    let out = output("ss", &["-ltnpH"]).await?;
    Some(ss_ports(&out, &pids))
}

/// A process listening on a TCP port (Go's `portholder.Holder`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    pub command: String,
    /// As the system reports it: `127.0.0.1:5432`, `*:5432`.
    pub addr: String,
}

/// Who listens on TCP `port` (Go's `portholder.Who`), so a person is told who to stop and not just that the bind
/// failed. `None` when nothing does, or when the system cannot say (no `lsof`, a process it may not look at): an
/// unknown holder is never invented.
pub async fn who(port: u16) -> Option<Holder> {
    let spec = format!("-iTCP:{port}");
    let out = output_any("lsof", &["-nP", &spec, "-sTCP:LISTEN", "-Fpcn"]).await?;
    parse_holder(&out)
}

// fuzz:parser fuzz_porttrap
/// lsof's `-F` output: `p<pid>`, `c<command>`, `n<address>` lines, a set per process; the first process is enough.
pub fn parse_holder(out: &str) -> Option<Holder> {
    let mut h = Holder {
        pid: 0,
        command: String::new(),
        addr: String::new(),
    };
    for line in out.lines().filter(|l| l.len() >= 2) {
        let mut chars = line.chars();
        let (tag, rest) = (chars.next(), chars.as_str());
        match tag {
            Some('p') if h.pid != 0 => break,
            Some('p') => h.pid = rest.parse().unwrap_or(0),
            Some('c') => h.command = rest.into(),
            Some('n') if h.addr.is_empty() => h.addr = rest.trim().into(),
            _ => {}
        }
    }
    (h.pid != 0).then_some(h)
}

async fn output(prog: &str, args: &[&str]) -> Option<String> {
    let out = run(prog, args).await?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn output_any(prog: &str, args: &[&str]) -> Option<String> {
    let out = run(prog, args).await?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn run(prog: &str, args: &[&str]) -> Option<std::process::Output> {
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    tokio::time::timeout(ASK, cmd.output()).await.ok()?.ok()
}

// fuzz:parser fuzz_porttrap
/// `root` and every process below it, from `ps -o pid= -o ppid=`, breadth first.
pub fn descendants(root: u32, ps: &str) -> Vec<u32> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for line in ps.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let [pid, ppid] = f.as_slice() else {
            continue;
        };
        if let (Ok(pid), Ok(ppid)) = (pid.parse::<u32>(), ppid.parse::<u32>())
            && pid != ppid
        {
            children.entry(ppid).or_default().push(pid);
        }
    }
    let mut seen = BTreeSet::from([root]);
    let mut out = vec![root];
    let mut i = 0;
    while let Some(p) = out.get(i).copied() {
        for c in children.get(&p).into_iter().flatten() {
            if seen.insert(*c) {
                out.push(*c);
            }
        }
        i += 1;
    }
    out
}

// fuzz:parser fuzz_porttrap
/// The ports in `lsof -Fn` output (`n<addr>:<port>` lines), each once.
pub fn lsof_ports(out: &str) -> Vec<u16> {
    uniq(out.lines().filter_map(|l| l.strip_prefix('n')).map(port_of))
}

// fuzz:parser fuzz_porttrap
/// The ports `ss -ltnpH` lists for any of `pids`, each once.
pub fn ss_ports(out: &str, pids: &[u32]) -> Vec<u16> {
    let ports = out.lines().filter_map(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        let local = f.get(3).filter(|_| f.len() >= 6)?;
        let ours = line
            .split("pid=")
            .skip(1)
            .filter_map(|rest| {
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                digits.parse::<u32>().ok()
            })
            .any(|pid| pids.contains(&pid));
        ours.then(|| port_of(local))
    });
    uniq(ports)
}

fn port_of(addr: &str) -> u16 {
    addr.rsplit_once(':')
        .and_then(|(_, p)| p.parse().ok())
        .unwrap_or(0)
}

fn uniq(ports: impl Iterator<Item = u16>) -> Vec<u16> {
    // Sorted, as Go's `uniq` sorts: the hint names them in port order.
    ports
        .filter(|p| *p > 0)
        .collect::<BTreeSet<u16>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Go's `TestPortTrapHint`, case for case, byte for byte.
    #[test]
    fn the_hint_names_where_it_listens() {
        let tail = "that doze gave it; start it with --port $PORT (vite: --port $PORT --strictPort), or set direct = \
                    true to have doze leave its port alone";
        assert_eq!(
            hint("web", 41234, &[5173]),
            format!("web listens on :5173, not on $PORT (41234) {tail}")
        );
        assert_eq!(
            hint("web", 41234, &[3000, 24678]),
            format!("web listens on :3000, :24678, not on $PORT (41234) {tail}")
        );
        assert_eq!(
            hint("web", 41234, &[]),
            "web started but listens on no port; doze waited for $PORT (41234)"
        );
        assert_eq!(hint("web", 41234, &[41234, 9229]), "");
        assert_eq!(hint("web", 0, &[5173]), "");
    }

    /// Go's `TestEnvPort`.
    #[test]
    fn the_port_doze_gave() {
        let env = |kv: &[(&str, &str)]| -> BTreeMap<String, String> {
            kv.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
        };
        assert_eq!(env_port(&env(&[("HOME", "/x"), ("PORT", "41234")])), 41234);
        assert_eq!(env_port(&env(&[("APORT", "1"), ("PORTS", "2")])), 0);
    }

    /// Go's `TestParse`.
    #[test]
    fn the_first_holder_is_read() {
        let h =
            parse_holder("p412\ncpostgres\nn127.0.0.1:5432\nn[::1]:5432\np500\ncother\nn*:5432\n");
        assert_eq!(
            h,
            Some(Holder {
                pid: 412,
                command: "postgres".into(),
                addr: "127.0.0.1:5432".into()
            })
        );
        assert_eq!(parse_holder(""), None);
        // Found by the fuzzer: a line starting with a character of several bytes.
        assert_eq!(parse_holder("P\n\u{fffd}x\n"), None);
    }

    /// Go's `TestWhoFindsTheListener`: this very process, on a port it listens on.
    #[tokio::test]
    async fn who_finds_the_listener() {
        if doze_os::which::which("lsof").is_none() {
            return;
        }
        let Ok(l) = std::net::TcpListener::bind("127.0.0.1:0") else {
            return;
        };
        let port = l.local_addr().map(|a| a.port()).unwrap_or(0);
        let h = who(port).await;
        assert_eq!(h.map(|h| h.pid), Some(std::process::id()));
    }

    /// Go's portholder parsers: the tree from `ps`, lsof's and ss's listings.
    #[test]
    fn the_system_listings_are_read_as_go_reads_them() {
        let ps = "  1     0\n 10     1\n 11    10\n 12    11\n 20     1\n 13    10\n bad\n 14 14\n";
        assert_eq!(descendants(10, ps), [10, 11, 13, 12]);
        assert_eq!(descendants(99, ps), [99]);
        assert_eq!(
            lsof_ports("p10\nf5\nn127.0.0.1:5173\nf6\nn*:5173\nn[::1]:3000\n"),
            [3000, 5173]
        );
        let ss = "LISTEN 0 511 127.0.0.1:5173 0.0.0.0:* users:((\"node\",pid=11,fd=20))\n\
                  LISTEN 0 128 0.0.0.0:22 0.0.0.0:* users:((\"sshd\",pid=1,fd=3))\n\
                  LISTEN 0 511 [::1]:3000 [::]:* users:((\"node\",pid=13,fd=21),(\"x\",pid=2,fd=1))\n";
        assert_eq!(ss_ports(ss, &[10, 11, 13]), [3000, 5173]);
    }
}
