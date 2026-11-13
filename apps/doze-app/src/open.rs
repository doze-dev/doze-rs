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

/// A folder argument made absolute (the app may run from elsewhere); links and flags as they are.
pub(crate) fn absolute(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--show" {
            out.push(a.clone());
            out.extend(it.next().cloned());
        } else if a.starts_with('-') || crate::link::is(a) {
            out.push(a.clone());
        } else {
            let p = std::path::absolute(a).unwrap_or_else(|_| PathBuf::from(a));
            out.push(p.to_string_lossy().into_owned());
        }
    }
    out
}

pub(crate) fn run(mut a: cli::App) -> ExitCode {
    a.forward = absolute(&a.forward);
    // No folder and no link: the folder this is run in, when it has a doze.hcl (Go: the doze.hcl here).
    let named = names_a_place(&a.forward);
    if !named
        && std::path::Path::new("doze.hcl").is_file()
        && let Ok(here) = std::env::current_dir()
    {
        a.forward.insert(0, here.to_string_lossy().into_owned());
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
        .filter(|p| !crate::around::bundled() && p.is_file());
    Ok(beside.unwrap_or(me))
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
        let out = absolute(&args);
        assert!(
            out.first()
                .is_some_and(|p| p.starts_with('/') && p.ends_with("/shop"))
        );
        assert_eq!(out.get(1..), args.get(1..));
    }
}
