//! The look at the machine: only reads. `ps` for this person's processes, the home's state folders and their sizes,
//! the routes record beside `/etc/resolver`, and whether this machine trusts doze's authority (`security
//! verify-cert`, which asks the keychain and changes nothing).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Authority, Machine, Proc, StateDir};
use crate::view::StackView;

/// Looks at this machine.
pub fn gather(home: &Path, stacks: &[StackView], admin: bool) -> Machine {
    let uid = doze_os::process::uid().to_string();
    let ps = Command::new("ps")
        .args(["-U", &uid, "-o", "pid=,ppid=,lstart=,command="])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let user_home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
    let mut m = gather_with(home, stacks, &ps, Path::new("/etc/resolver"), &trusted);
    m.admin = admin;
    m.user_home = user_home;
    m
}

/// Looks, given what `ps` printed, where the routes live, and how to ask whether a root is trusted.
pub fn gather_with(
    home: &Path,
    stacks: &[StackView],
    ps: &str,
    resolver: &Path,
    is_trusted: &dyn Fn(&Path) -> bool,
) -> Machine {
    let known = stacks
        .iter()
        .filter(|s| !s.dir.is_empty())
        .map(|s| canon(&Path::new(&s.dir).join("doze.hcl")))
        .collect();
    let procs = parse_ps(ps)
        .into_iter()
        .map(|mut p| {
            if !p.daemon.is_empty() {
                p.config_there = Path::new(&p.daemon).is_file();
                p.daemon = canon(Path::new(&p.daemon));
            }
            p.exe = canon(Path::new(&p.exe));
            p
        })
        .collect();
    let records = super::read_routes(home);
    let routes = records
        .iter()
        .filter(|(d, dirs)| !declared(dirs) && routed(resolver, d))
        .map(|(d, _)| d.clone())
        .collect();
    let ca = home.join("ca");
    let authority = ca.join("root.crt").is_file().then(|| Authority {
        trusted: is_trusted(&ca.join("root.crt")),
        used: records.values().any(|dirs| declared(dirs)),
        dir: ca,
    });
    Machine {
        home: PathBuf::from(canon(home)),
        known,
        procs,
        dirs: state_dirs(home),
        routes,
        authority,
        admin: false,
        user_home: None,
    }
}

/// The folders under `<home>/projects`, with their sizes when nothing claims them.
fn state_dirs(home: &Path) -> Vec<StateDir> {
    let Ok(rd) = std::fs::read_dir(home.join("projects")) else {
        return Vec::new();
    };
    let mut out: Vec<StateDir> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .map(|dir| {
            let listed = dir.join("project.json").is_file();
            let running = doze_projects::daemon_pid(&doze_projects::StackKey::new(&dir))
                .is_some_and(|p| p.alive);
            let size = if listed || running {
                String::new()
            } else {
                super::size_words(du(&dir))
            };
            StateDir {
                dir,
                listed,
                running,
                size,
            }
        })
        .collect();
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

/// The bytes of every file under `dir`; links are not followed.
fn du(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.filter_map(Result::ok)
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => du(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// One of the folders still has a doze.hcl: a claimant that may still mean it (Go `declared`).
fn declared(dirs: &[String]) -> bool {
    dirs.iter().any(|d| Path::new(d).join("doze.hcl").is_file())
}

/// The domain's resolver file is doze's: it sends the domain to the local resolver.
fn routed(resolver: &Path, domain: &str) -> bool {
    std::fs::read_to_string(resolver.join(domain))
        .is_ok_and(|t| t.lines().any(|l| l.trim() == "nameserver 127.0.0.1"))
}

/// Whether this Mac trusts the root as an anchor (Go `rootTrusted`). Elsewhere doze trusts nothing in a store the
/// app can ask, so it is not.
fn trusted(root: &Path) -> bool {
    std::env::consts::OS == "macos"
        && Command::new("/usr/bin/security")
            .args(["verify-cert", "-L", "-l", "-q", "-p", "basic", "-c"])
            .arg(root)
            .output()
            .is_ok_and(|o| o.status.success())
}

/// A path with its links followed (`/var` is `/private/var` on a Mac), so two spellings of one file compare equal.
pub(super) fn canon(p: &Path) -> String {
    std::fs::canonicalize(p)
        .or_else(|_| std::path::absolute(p))
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// The processes `ps -o pid=,ppid=,lstart=,command=` printed (Go `processes`): `pid ppid Day Mon DD HH:MM:SS YYYY
/// command…`. A line that does not read so is skipped.
// fuzz:parser fuzz_ps
pub fn parse_ps(text: &str) -> Vec<Proc> {
    text.lines().filter_map(ps_line).collect()
}

fn ps_line(line: &str) -> Option<Proc> {
    let f: Vec<&str> = line.split_whitespace().collect();
    let (pid, ppid) = (f.first()?.parse().ok()?, f.get(1)?.parse().ok()?);
    let clock = f.get(5)?;
    let mut p = Proc {
        pid,
        ppid,
        since: format!(
            "{} {} {}",
            f.get(3)?,
            f.get(4)?,
            clock.get(..5).unwrap_or(clock)
        ),
        exe: (*f.get(7)?).to_owned(),
        ..Proc::default()
    };
    let args = f.get(8..).unwrap_or_default();
    if let Some(i) = args.iter().position(|a| *a == "__daemon") {
        let rest = args.get(i + 1..).unwrap_or_default();
        for w in rest.windows(2) {
            if let [flag, value] = w
                && *flag == "--config"
            {
                p.daemon = (*value).to_owned();
            }
        }
    }
    Some(p)
}
