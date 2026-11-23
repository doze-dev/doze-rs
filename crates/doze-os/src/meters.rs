//! What processes cost (Go's `ui.ProcStats` and `ui.ProcTree`): for each root, its own resident memory, the CPU of it
//! and everything under it (a database's per-connection children do the work), and the processes under it.
//!
//! On macOS and Linux only the roots' own trees are read, in-process: libproc's child list and task info on macOS,
//! `/proc` on Linux. CPU is the change in each process's CPU time since the last reading, so it is what the
//! process does now, not its lifetime average; a root seen for the first time is read twice, a moment apart. Elsewhere
//! one `ps -Ao pid=,ppid=,rss=,%cpu=,comm=` covers every root, as Go's daemon read it (a fork that reads every process
//! of the machine, ~25 ms of CPU on a desktop with 800). Windows has no `ps`: nothing is measured there (Go read RSS
//! alone and got none either).

use std::collections::{BTreeMap, BTreeSet};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod rate;
#[cfg(target_os = "linux")]
use linux as native;
#[cfg(target_os = "macos")]
use macos as native;

/// How long apart the two readings of a root seen for the first time are.
#[cfg(any(target_os = "macos", target_os = "linux"))]
const FIRST_LOOK: std::time::Duration = std::time::Duration::from_millis(250);

/// The most processes read under the roots together (a fork bomb is not walked to its end).
#[cfg(any(target_os = "macos", target_os = "linux"))]
const MOST: usize = 4096;

/// One process in the table.
#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    /// Resident bytes.
    pub rss: u64,
    /// Percent of one core.
    pub cpu: f64,
    /// The program's name (its path's last part).
    pub cmd: String,
}

/// One process under a root, `depth` 1 for a direct child.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub proc: Proc,
    pub depth: u32,
}

/// A root's meters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meters {
    /// The root's own resident bytes.
    pub rss: u64,
    /// The CPU of the root and everything under it, in percent of one core.
    pub cpu: f64,
    /// Everything under it, depth first.
    pub children: Vec<Node>,
}

/// The meters of each root that is running now; a root that is not in the table is left out.
pub async fn sample(roots: &[u32]) -> BTreeMap<u32, Meters> {
    if roots.is_empty() {
        return BTreeMap::new();
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if let Some(m) = in_process(roots).await {
        return m;
    }
    match table().await {
        Some(text) => of_roots(&parse(&text), roots),
        None => BTreeMap::new(),
    }
}

/// One process read in-process: its CPU time so far, which [`rate`] turns into a share of a core.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Debug, Clone, PartialEq)]
struct Raw {
    pid: u32,
    /// 0 where the reader does not know it; the walk fills it in.
    ppid: u32,
    rss: u64,
    cpu_ns: u64,
    cmd: String,
}

/// The roots' meters read without `ps`, or `None` where the system cannot (a Linux without `/proc` or its child lists).
#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn in_process(roots: &[u32]) -> Option<BTreeMap<u32, Meters>> {
    let mut raws = trees(roots)?;
    let new_root = roots
        .iter()
        .any(|r| !rate::known(*r) && raws.iter().any(|p| p.pid == *r));
    if new_root {
        // A first reading has nothing to compare with: take it as the start, and read again a moment later.
        let _ = rate::rated(raws, std::time::Instant::now());
        tokio::time::sleep(FIRST_LOOK).await;
        raws = trees(roots)?;
    }
    let mut procs = rate::rated(raws, std::time::Instant::now());
    procs.sort_by_key(|p| p.pid);
    Some(of_roots(&procs, roots))
}

/// Every process under `roots` and the roots themselves, each read once; a parent the reader does not tell is the one
/// the walk came from.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn trees(roots: &[u32]) -> Option<Vec<Raw>> {
    let mut reader = native::Reader::new()?;
    let mut out: Vec<Raw> = Vec::new();
    let mut at: BTreeMap<u32, usize> = BTreeMap::new();
    let mut todo: Vec<(u32, u32)> = roots.iter().map(|r| (*r, 0)).collect();
    while let Some((pid, parent)) = todo.pop() {
        if let Some(i) = at.get(&pid) {
            // A root under another root: read as a root first, its parent is learned now.
            if let Some(p) = out.get_mut(*i)
                && p.ppid == 0
            {
                p.ppid = parent;
            }
            continue;
        }
        if out.len() >= MOST {
            break;
        }
        let Some(mut p) = reader.read(pid) else {
            continue;
        };
        if p.ppid == 0 {
            p.ppid = parent;
        }
        todo.extend(reader.children(pid).into_iter().map(|c| (c, pid)));
        at.insert(pid, out.len());
        out.push(p);
    }
    Some(out)
}

#[cfg(unix)]
async fn table() -> Option<String> {
    let out = tokio::process::Command::new("ps")
        .args(["-Ao", "pid=,ppid=,rss=,%cpu=,comm="])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(windows)]
async fn table() -> Option<String> {
    None
}

// fuzz:parser fuzz_ps
/// The process table `ps -Ao pid=,ppid=,rss=,%cpu=,comm=` printed (RSS in KiB). A line that does not read is skipped;
/// a command may hold spaces.
pub fn parse(text: &str) -> Vec<Proc> {
    text.lines().filter_map(line).collect()
}

fn line(l: &str) -> Option<Proc> {
    let mut f = l.split_whitespace();
    let pid = f.next()?.parse().ok()?;
    let ppid = f.next()?.parse().ok()?;
    let kib: u64 = f.next()?.parse().ok()?;
    let cpu: f64 = f.next()?.replace(',', ".").parse().ok()?;
    let cmd = command(&f.collect::<Vec<_>>().join(" "));
    Some(Proc {
        pid,
        ppid,
        rss: kib.saturating_mul(1024),
        cpu: if cpu.is_finite() { cpu.max(0.0) } else { 0.0 },
        cmd,
    })
}

/// A command as the table shows it: its words joined by one space, and of a path only the last part.
fn command(s: &str) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.rsplit('/').next().unwrap_or_default().to_string()
}

/// Each root's meters from `procs`.
pub fn of_roots(procs: &[Proc], roots: &[u32]) -> BTreeMap<u32, Meters> {
    let by_pid: BTreeMap<u32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut kids: BTreeMap<u32, Vec<&Proc>> = BTreeMap::new();
    for p in procs {
        kids.entry(p.ppid).or_default().push(p);
    }
    let mut out = BTreeMap::new();
    for &root in roots {
        let Some(r) = by_pid.get(&root) else {
            continue;
        };
        let mut m = Meters {
            rss: r.rss,
            cpu: r.cpu,
            children: Vec::new(),
        };
        // Depth first, each child before its own children (Go's walk); a pid seen twice (a loop in a torn table)
        // is not walked again.
        let mut seen = BTreeSet::from([root]);
        let mut stack: Vec<(&Proc, u32)> = kids
            .get(&root)
            .into_iter()
            .flatten()
            .rev()
            .map(|c| (*c, 1))
            .collect();
        while let Some((c, depth)) = stack.pop() {
            if !seen.insert(c.pid) {
                continue;
            }
            m.cpu += c.cpu;
            m.children.push(Node {
                proc: c.clone(),
                depth,
            });
            stack.extend(
                kids.get(&c.pid)
                    .into_iter()
                    .flatten()
                    .rev()
                    .map(|k| (*k, depth + 1)),
            );
        }
        out.insert(root, m);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = "    1     0  1024   0.0 /sbin/launchd
  100     1  2048   1.5 /usr/local/bin/postgres
  101   100   512   2.0 postgres: checkpointer
  102   100   256  10,5 postgres
  103   101   128   0.5 /bin/sh
  200     1    64   0.0 other
  bad line
";

    #[test]
    fn a_root_has_its_own_memory_and_its_trees_cpu_depth_first() {
        let procs = parse(PS);
        assert_eq!(procs.len(), 6);
        assert_eq!(procs[2].cmd, "postgres: checkpointer");
        let m = of_roots(&procs, &[100, 999]);
        assert_eq!(m.len(), 1, "a root that is gone is left out");
        let pg = &m[&100];
        assert_eq!(pg.rss, 2048 * 1024, "the root's own RSS, as Go");
        assert!((pg.cpu - 14.5).abs() < 1e-9, "the tree's CPU: {}", pg.cpu);
        let order: Vec<(u32, u32)> = pg.children.iter().map(|n| (n.proc.pid, n.depth)).collect();
        assert_eq!(order, [(101, 1), (103, 2), (102, 1)]);
        assert_eq!(pg.children[1].proc.cmd, "sh");
    }

    #[test]
    fn a_loop_in_the_table_is_walked_once() {
        let procs = parse("5 6 1 1.0 a\n6 5 1 1.0 b\n");
        let m = of_roots(&procs, &[5]);
        assert_eq!(m[&5].children.len(), 1);
    }

    /// A shell that spins under a shell that waits: the root's CPU is its tree's, as it is now, and the spinner is
    /// shown under it with its command.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    async fn a_busy_child_shows_as_busy_now() {
        let mut root = std::process::Command::new("/bin/sh")
            .args(["-c", "/bin/sh -c 'while :; do :; done' & wait"])
            .spawn()
            .unwrap();
        let pid = root.id();
        for _ in 0..40 {
            let m = sample(&[pid]).await;
            if m.get(&pid).is_some_and(|m| !m.children.is_empty()) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let m = sample(&[pid]).await;
        for c in &m[&pid].children {
            let _ = std::process::Command::new("kill")
                .arg(c.proc.pid.to_string())
                .status();
        }
        let _ = root.kill();
        let _ = root.wait();
        let r = &m[&pid];
        assert_eq!(r.children.len(), 1, "{r:?}");
        assert_eq!(r.children[0].proc.cmd, "sh");
        assert_eq!(r.children[0].proc.ppid, pid);
        assert!(r.cpu > 30.0, "a spinning child is busy now: {r:?}");
    }

    #[tokio::test]
    async fn this_process_is_measured_where_there_is_ps() {
        let me = std::process::id();
        let m = sample(&[me]).await;
        if cfg!(windows) {
            assert!(m.is_empty());
        } else {
            assert!(m[&me].rss > 0, "{m:?}");
        }
    }
}
