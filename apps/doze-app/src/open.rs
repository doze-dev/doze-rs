//! `doze-app open [FOLDER | doze://…] [--show NAME]` (Go `cmd/doze/open.go`, PLAN.md row 89 and step 3.5): the app, at
//! this project. An app already running (the bundle, or any launch holding the doze home) is handed the arguments and
//! goes there; otherwise, from a terminal, the app is started on its own (its own session, its output in
//! `$DOZE_HOME/desk/app.log`) so the prompt comes back; elsewhere (Finder, launchd, a script with no terminal) it runs
//! in place. Either way a terminal is told "✓ doze is open".
//!
//! Which binary is started: the one in `$DOZE_APP_BUNDLE/Contents/MacOS/` when that names a Doze.app (so the app runs
//! from its bundle and the system's notifications work); else, for a build in a target folder, the Doze.app
//! `cargo xtask app-bundle` made beside it (`target/bundle`); else this one. Nothing looks in /Applications: until the
//! Rust app ships there, the Doze.app in /Applications is the Go app.

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use crate::cli;

/// How long a started app is watched for failing at once (a bad flag, no display) before the terminal is told.
const SETTLE: Duration = Duration::from_millis(700);
/// Marks the started copy, so it runs rather than starting another.
const CHILD: &str = "DOZE_APP_CHILD";
const OPEN: &str =
    "✓ doze is open · the bat stays in the menu bar; stacks keep running when it quits";

/// A launch's arguments as Go's `dirFrom` reads a second launch's: a folder made absolute against `cwd`, the folder it
/// ran in (the app that hears it runs elsewhere); with no folder or link named, the project `cwd` is inside, found
/// upward as the CLI finds doze.hcl, else that folder itself (Go). Links, flags and `--show NAME` stay as they are.
pub(crate) fn resolved(args: &[String], cwd: &Path) -> Vec<String> {
    let mut out = absolute(args, cwd);
    if !names_a_place(&out) {
        out.insert(0, project_dir(cwd).to_string_lossy().into_owned());
    }
    out
}

/// The nearest folder at or above `dir` with a doze.hcl, or `dir` itself when none has one (Go `projectDir`): the
/// folder then opens on its own page, to set it up.
fn project_dir(dir: &Path) -> PathBuf {
    dir.ancestors()
        .find(|d| d.join("doze.hcl").is_file())
        .unwrap_or(dir)
        .to_path_buf()
}

/// A folder argument made absolute against `cwd`; links and flags as they are.
fn absolute(args: &[String], cwd: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--show" {
            out.push(a.clone());
            out.extend(it.next().cloned());
        } else if a.starts_with('-') || crate::link::is(a) {
            out.push(a.clone());
        } else {
            out.push(cwd.join(a).to_string_lossy().into_owned());
        }
    }
    out
}

pub(crate) fn run(mut a: cli::App) -> ExitCode {
    // No folder and no link: the project the folder this is run in is inside (Go: doze.hcl found upward).
    if let Ok(here) = std::env::current_dir() {
        a.forward = resolved(&a.forward, &here);
    }
    let home = match doze_os::home::doze_home() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("doze-app open: no doze home: {e}");
            return ExitCode::FAILURE;
        }
    };
    let child = std::env::var_os(CHILD).is_some();
    let fixture = a.from.fixture.is_some() || a.from.synth.is_some();
    if !child && !fixture {
        match crate::single::hand_over(&home, &a.forward) {
            Ok(true) => return said_open(),
            Ok(false) => {}
            Err(e) => {
                eprintln!("doze-app open: the running app did not answer: {e}");
                return ExitCode::FAILURE;
            }
        }
        if std::io::stdin().is_terminal() {
            let mut args = a.forward.clone();
            if a.menu_bar {
                args.push("--menu-bar".into());
            }
            return match detach(&home, &args) {
                Ok(()) => said_open(),
                Err(e) => {
                    eprintln!("doze-app open: {e}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    crate::app::run(a)
}

/// Whether the arguments name a folder or a link (the value after `--show` is a service, not a place).
fn names_a_place(args: &[String]) -> bool {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--show" {
            it.next();
        } else if !a.starts_with('-') {
            return true;
        }
    }
    false
}

fn said_open() -> ExitCode {
    if std::io::stdout().is_terminal() {
        println!("{OPEN}");
    }
    ExitCode::SUCCESS
}

/// The binary to start: the bundle's, when `$DOZE_APP_BUNDLE` names one; else this one.
fn program() -> std::io::Result<PathBuf> {
    if let Some(b) = std::env::var_os("DOZE_APP_BUNDLE") {
        let exe = Path::new(&b).join("Contents/MacOS/doze-app");
        if exe.is_file() {
            return Ok(exe);
        }
        return Err(std::io::Error::other(format!(
            "DOZE_APP_BUNDLE names no Doze.app: {} is not there",
            exe.display()
        )));
    }
    let me = std::env::current_exe()?;
    // A build run from the target folder: the Doze.app `cargo xtask app-bundle` made beside it (`target/bundle`), so
    // the app it starts has a bundle and the system's notifications.
    let beside = me
        .parent()
        .and_then(Path::parent)
        .map(|t| t.join("bundle/Doze.app/Contents/MacOS/doze-app"))
        .filter(|p| !crate::around::bundled() && newer_or_same(p, &me));
    Ok(beside.unwrap_or(me))
}

/// `bundled` was built no earlier than `me`: a bundle older than this build would run old code without a word.
fn newer_or_same(bundled: &Path, me: &Path) -> bool {
    let at = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    matches!((at(bundled), at(me)), (Some(b), Some(m)) if b >= m)
}

/// Starts the app on its own and returns without waiting, unless it fails at once.
fn detach(home: &Path, args: &[String]) -> Result<(), String> {
    let exe = program().map_err(|e| e.to_string())?;
    let dir = home.join("desk");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join("app.log");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let from = log.metadata().map(|m| m.len()).unwrap_or(0);
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let mut c = Command::new(&exe);
    c.args(args)
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err);
    // Its own session, as Go's detachAttr: the terminal's hang-up does not reach it.
    let mut child =
        doze_os::detach::spawn_detached_child(c).map_err(|e| format!("{}: {e}", exe.display()))?;
    let start = Instant::now();
    while start.elapsed() < SETTLE {
        match child.try_wait() {
            Ok(Some(s)) if s.success() => return Ok(()),
            Ok(Some(s)) => {
                return Err(format!(
                    "the app stopped at once ({s}){}",
                    tail(&path, from)
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

/// The last lines written to the log after `from`.
fn tail(path: &Path, from: u64) -> String {
    let text = std::fs::read(path).unwrap_or_default();
    let new = text
        .get(usize::try_from(from).unwrap_or(usize::MAX)..)
        .unwrap_or_default();
    let s = String::from_utf8_lossy(new);
    let lines: Vec<&str> = s.trim().lines().collect();
    let last = lines
        .get(lines.len().saturating_sub(8)..)
        .unwrap_or_default();
    if last.is_empty() {
        String::new()
    } else {
        format!(":\n  {}", last.join("\n  "))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::absolute;

    #[test]
    fn the_value_of_show_is_not_a_place() {
        let v = |s: &[&str]| s.iter().map(|x| (*x).to_owned()).collect::<Vec<String>>();
        assert!(!super::names_a_place(&v(&["--show", "api"])));
        assert!(super::names_a_place(&v(&["--show", "api", "/code/shop"])));
        assert!(super::names_a_place(&v(&["doze://shop"])));
        assert!(!super::names_a_place(&[]));
    }

    #[test]
    fn folders_become_absolute_and_links_and_show_stay() {
        let args: Vec<String> = ["shop", "--show", "api", "doze://shop/db"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let out = absolute(&args, Path::new("/code"));
        assert_eq!(out.first().map(String::as_str), Some("/code/shop"));
        assert_eq!(out.get(1..), args.get(1..));
    }
}

#[cfg(test)]
mod upward {
    use super::resolved;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn a_launch_inside_a_project_opens_the_project_found_upward() {
        let root = std::env::temp_dir().join(format!("doze-app-upward-{}", std::process::id()));
        let deep = root.join("shop/src/api");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(root.join("shop/doze.hcl"), "stack = \"shop\"\n").unwrap();
        let shop = root.join("shop").to_string_lossy().into_owned();
        // Bare --show: the project around the folder it ran in, then the instance.
        assert_eq!(
            resolved(&v(&["--show", "db"]), &deep),
            v(&[&shop, "--show", "db"])
        );
        // A relative folder is the folder it ran in's, not the app's.
        assert_eq!(
            resolved(&v(&["api"]), &root.join("shop/src")),
            v(&[&format!("{shop}/src/api")])
        );
        // A link names its place; nothing is added.
        assert_eq!(resolved(&v(&["doze://shop"]), &deep), v(&["doze://shop"]));
        // No project above: the folder itself, as Go's projectDir.
        assert_eq!(resolved(&[], &root), v(&[&root.to_string_lossy()]));
        let _ = std::fs::remove_dir_all(&root);
    }
}
