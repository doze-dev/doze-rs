//! Reading a process on Linux from `/proc`: its children from each thread's `children` list, its parent, CPU time,
//! resident memory and command (the kernel's `comm`, as `ps -o comm` shows it) from its `stat`.

use super::Raw;
use std::path::Path;

/// Reads processes, knowing the clock tick and page size `stat` counts in.
#[derive(Debug)]
pub(super) struct Reader {
    tick_ns: u64,
    page: u64,
}

impl Reader {
    /// `None` without `/proc` or without its child lists (a kernel built without `CONFIG_PROC_CHILDREN`).
    pub(super) fn new() -> Option<Reader> {
        let me = std::process::id();
        if !Path::new(&format!("/proc/{me}/task/{me}/children")).exists() {
            return None;
        }
        let hz = rustix::param::clock_ticks_per_second().max(1);
        Some(Reader {
            tick_ns: 1_000_000_000 / hz,
            page: u64::try_from(rustix::param::page_size()).unwrap_or(4096),
        })
    }

    /// The pids whose parent is `pid` (each of its threads keeps the children it started).
    #[allow(clippy::unused_self)] // WHY: the same shape as the macOS reader's
    pub(super) fn children(&self, pid: u32) -> Vec<u32> {
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for t in tasks.flatten() {
            if let Ok(s) = std::fs::read_to_string(t.path().join("children")) {
                out.extend(s.split_whitespace().filter_map(|p| p.parse::<u32>().ok()));
            }
        }
        out
    }

    /// `pid`'s parent, memory, CPU time and command; `None` once it is gone.
    pub(super) fn read(&mut self, pid: u32) -> Option<Raw> {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat(&s, self.tick_ns, self.page)
    }
}

/// One `/proc/<pid>/stat` line: `pid (comm) state ppid ...`, where the command may hold spaces and parentheses, so the
/// fields after it are found from its last `)`. CPU time is fields 14 and 15 in clock ticks, resident memory field 24
/// in pages.
fn stat(s: &str, tick_ns: u64, page: u64) -> Option<Raw> {
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    let pid = s.get(..open)?.trim().parse().ok()?;
    let comm = s.get(open + 1..close)?;
    let f: Vec<&str> = s.get(close + 1..)?.split_whitespace().collect();
    let num = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok());
    let ppid = u32::try_from(num(1)?).ok()?;
    let ticks = num(11)?.saturating_add(num(12)?);
    Some(Raw {
        pid,
        ppid,
        rss: num(21)?.saturating_mul(page),
        cpu_ns: ticks.saturating_mul(tick_ns),
        cmd: super::command(comm),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_stat_line_is_read_past_a_command_with_parentheses() {
        let line = "4242 (my (odd) cmd) S 7 4242 4242 0 -1 4194560 100 0 0 0 150 50 0 0 20 0 3 0 12345 99999 256 18446744073709551615";
        let r = stat(line, 10_000_000, 4096).unwrap();
        assert_eq!((r.pid, r.ppid), (4242, 7));
        assert_eq!(r.cmd, "my (odd) cmd");
        assert_eq!(r.cpu_ns, 200 * 10_000_000);
        assert_eq!(r.rss, 256 * 4096);
        assert!(stat("4242 (x) S", 1, 1).is_none());
    }

    #[test]
    fn children_and_this_process_are_read() {
        let Some(mut r) = Reader::new() else {
            eprintln!("skipped: no child lists in /proc");
            return;
        };
        let mut kid = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let me = std::process::id();
        assert!(r.children(me).contains(&kid.id()));
        let k = r.read(kid.id()).unwrap();
        assert_eq!((k.ppid, k.cmd.as_str()), (me, "sleep"));
        assert!(r.read(me).unwrap().rss > 0);
        let _ = kid.kill();
        let _ = kid.wait();
    }
}
