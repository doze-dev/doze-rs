//! What stacks left behind, counted (Go `serve/leftovers.go`, `leftoversWithin`): a stack's daemon running for a
//! doze.hcl Home does not list, an engine plugin whose daemon is gone (its parent is pid 1), and a state folder under
//! the home that no stack owns. Needs you says "N things left behind" and "N data folders no stack uses".
//!
//! This only reads: the process table (`ps`, this user's processes) and the home's folders. Clearing a leftover is
//! Settings › Machine's, asked one at a time; nothing here stops a process or deletes a file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use doze_projects::StackKey;

/// One process, as `ps` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Proc {
    ppid: u32,
    exe: String,
    /// The doze.hcl a stack's daemon runs (`__daemon --config FILE`), else empty.
    daemon: String,
}

/// The counts: what Clean up clears, and the data folders (deleted one at a time, after asking).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub safe: usize,
    pub data: usize,
}

/// A path with its links followed (`/var` is `/private/var` on a Mac), so two spellings of one file compare equal.
fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// `ps` output read into processes: `pid ppid lstart(5 words) command…`.
fn parse_ps(out: &str) -> Vec<Proc> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let ppid = f.get(1)?.parse().ok()?;
            f.first()?.parse::<u32>().ok()?;
            let exe = (*f.get(7)?).to_owned();
            let args = f.get(8..).unwrap_or_default();
            let daemon = args
                .iter()
                .position(|a| *a == "__daemon")
                .and_then(|i| {
                    let rest = args.get(i + 1..)?;
                    let c = rest.iter().position(|a| *a == "--config")?;
                    rest.get(c + 1).map(|s| (*s).to_owned())
                })
                .unwrap_or_default();
            Some(Proc { ppid, exe, daemon })
        })
        .collect()
}

/// An engine plugin in some doze home's module cache (`<home>/modules/<ns>/<name>/<version>/bin/<name>-plugin`).
fn is_plugin(exe: &str) -> bool {
    let parts: Vec<&str> = exe.split('/').collect();
    let n = parts.len();
    n >= 6
        && parts.get(n - 2) == Some(&"bin")
        && parts.get(n - 6) == Some(&"modules")
        && parts.last().is_some_and(|p| p.ends_with("-plugin"))
}

/// The leftovers among `procs` and the home's state folders, given the doze.hcl files of the stacks Home lists.
fn count_in(home: &Path, procs: &[Proc], known: &HashSet<PathBuf>) -> Counts {
    let mut c = Counts::default();
    let modules = canon(&home.join("modules"));
    for p in procs {
        if !p.daemon.is_empty() {
            if !known.contains(&canon(Path::new(&p.daemon))) {
                c.safe += 1;
            }
        } else if p.ppid == 1
            && (canon(Path::new(&p.exe)).starts_with(&modules) || is_plugin(&p.exe))
        {
            c.safe += 1;
        }
    }
    let Ok(dirs) = std::fs::read_dir(home.join("projects")) else {
        return c;
    };
    for d in dirs.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        if d.join(doze_projects::FILE_NAME).is_file() {
            continue; // a stack: the Library lists it, missing files and all
        }
        if doze_projects::daemon_pid(&StackKey::new(&d)).is_some_and(|p| p.alive) {
            continue; // running: counted above if it is not this Library's
        }
        c.data += 1;
    }
    c
}

/// Counts what stacks left behind on this machine and under `home`; `stack_files` are the doze.hcl files of the
/// stacks Home lists. Runs `ps` (read-only); call it off the UI thread.
#[must_use]
#[cfg(unix)]
pub fn count(home: &Path, stack_files: &[PathBuf]) -> Counts {
    use std::os::unix::fs::MetadataExt as _;
    // This user's processes: the home is theirs.
    let Ok(uid) = std::fs::metadata(home).map(|m| m.uid()) else {
        return Counts::default();
    };
    let out = Command::new("ps")
        .args(["-U", &uid.to_string(), "-o", "pid=,ppid=,lstart=,command="])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let known: HashSet<PathBuf> = stack_files.iter().map(|f| canon(f)).collect();
    count_in(home, &parse_ps(&out), &known)
}

/// Not counted where there is no `ps` to read.
#[must_use]
#[cfg(not(unix))]
pub fn count(_home: &Path, _stack_files: &[PathBuf]) -> Counts {
    Counts::default()
}

#[cfg(test)]
mod tests {
    use super::{is_plugin, parse_ps};

    #[test]
    fn the_process_table_is_read_for_daemons_and_orphaned_plugins() {
        let ps = "  412     1 Fri Nov 13 22:58:01 2026 /x/doze __daemon --config /a/shop/doze.hcl\n\
                  413   412 Fri Nov 13 22:58:01 2026 /h/modules/doze/postgres/0.2.3-aarch64/bin/postgres-plugin\n\
                  bad line\n";
        let p = parse_ps(ps);
        assert_eq!(p.len(), 2);
        assert_eq!(
            p.first().map(|p| p.daemon.as_str()),
            Some("/a/shop/doze.hcl")
        );
        assert_eq!(p.get(1).map(|p| p.ppid), Some(412));
        assert!(is_plugin(
            "/h/modules/doze/postgres/0.2.3-aarch64/bin/postgres-plugin"
        ));
        assert!(!is_plugin("/usr/bin/postgres"));
    }
}
