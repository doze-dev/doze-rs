//! `doze update`'s hand-over: each stack whose daemon still runs the old doze moves to the new one. Under the stack's
//! lock, what is awake is read from its daemon, the daemon is stopped as `down` stops it (its exact pid, only while it is
//! still the daemon), a new one is started from the new doze with the doze.hcl its record names (as `up` starts it),
//! and what was awake is woken again.
//!
//! The services stop for that moment: a hand-over that keeps them running needs the daemon to pass its listeners on,
//! which it does not do yet (compat.md rule 5). The new daemon has this command's environment, so `--var`s the stack
//! was started with are not carried over; the result says to `doze up` again for those.

use crate::cmd::down;
use doze_control::{Client, Verb};
use doze_daemon::ctl::{self, RunDir, StackLock};
use doze_projects::StackKey;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

const ASK: Duration = Duration::from_secs(30);

/// Why `exe` is not handed the stacks, or `None` when it runs and is this doze: `doze --json version` answers with
/// the envelope and a version. An older doze cannot (the Go one exits with "unknown flag: --json").
fn refused(exe: &Path) -> Option<String> {
    let answer = std::process::Command::new(exe)
        .args(["--json", "version"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok());
    let this_doze = answer.is_some_and(|v| {
        v.get("ok") == Some(&serde_json::Value::Bool(true))
            && v.pointer("/data/version")
                .is_some_and(serde_json::Value::is_string)
    });
    (!this_doze).then(|| {
        format!(
            "{} does not run as this doze (doze --json version did not answer; an older doze cannot); nothing was stopped",
            exe.display()
        )
    })
}

/// One stack's hand-over.
#[derive(Debug, Serialize)]
pub struct Moved {
    /// The stack's name (its folder's when none was recorded).
    pub stack: String,
    /// Whether it runs on the new doze now.
    pub moved: bool,
    /// What was woken again, or why it was not moved.
    pub detail: String,
}

/// What a stack's new daemon woke again.
enum Woke {
    All(Vec<String>),
    /// The new daemon runs; this wake failed.
    Partly(String),
}

/// Moves every running stack under `home` to the doze at `exe`. Nothing is stopped unless `exe` runs and is this
/// doze (`doze --json version` answers with its envelope): a broken install must not take the stacks down, and an
/// older doze (the Go one cannot answer `--json`) is never handed a stack. After a stack whose new daemon did not
/// start, the rest are left on the old doze.
pub(crate) fn all(home: &Path, exe: &Path) -> Vec<Moved> {
    let mut halted = refused(exe);
    let mut out = Vec::new();
    for r in doze_projects::running(home) {
        let stack = doze_projects::load(r.key.dir())
            .ok()
            .flatten()
            .map(|p| p.name)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| r.key.slug());
        // Checked before `one` runs: once halted, no other stack is touched.
        let result = match &halted {
            Some(why) => Err(Stop::Before(format!("left on the old doze: {why}"))),
            None => one(&r.key, exe),
        };
        let (moved, detail) = match result {
            Ok(Woke::All(w)) if w.is_empty() => (true, "nothing was awake".into()),
            Ok(Woke::All(w)) => (true, format!("woken again: {}", w.join(", "))),
            Ok(Woke::Partly(why)) => (true, why),
            Err(Stop::Before(why)) => (false, why),
            Err(Stop::After(why)) => {
                halted = Some(format!("{stack}'s new daemon did not start"));
                (false, why)
            }
        };
        out.push(Moved {
            stack,
            moved,
            detail,
        });
    }
    out
}

/// Why a stack was not moved: before its daemon was stopped (it runs on), or after (it is stopped).
enum Stop {
    Before(String),
    After(String),
}

/// Stops the stack's daemon and starts it again from `exe`; what it woke again.
fn one(key: &StackKey, exe: &Path) -> Result<Woke, Stop> {
    let config = doze_projects::load(key.dir())
        .ok()
        .flatten()
        .map(|p| p.config)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| {
            Stop::Before(
                "its record names no doze.hcl to start it with; doze down, then doze up, moves it"
                    .into(),
            )
        })?;
    let run = RunDir::of(key.dir());
    let _lock =
        StackLock::take(&run).map_err(|e| Stop::Before(format!("could not take its lock: {e}")))?;
    let client = Client::socket(key.socket())
        .named("cli")
        .with_timeout(Some(ASK));
    let snap = client
        .dial()
        .and_then(|c| c.snapshot())
        .map_err(|e| Stop::Before(format!("its daemon did not answer: {e}")))?;
    let awake: Vec<String> = snap
        .instances
        .iter()
        .filter(|v| v.declared && crate::cmd::status::awake(&v.state))
        .map(|v| v.name.clone())
        .collect();
    let pid = down::identify(key).ok_or_else(|| {
        Stop::Before("its pid file names no daemon doze started; it was left running".into())
    })?;
    if !down::stop(pid).map_err(|d| Stop::Before(d.message))? {
        return Err(Stop::Before(
            "its daemon is another user's; it was left running".into(),
        ));
    }
    down::clear(&down::leftovers(key));
    let start = ctl::Start {
        exe,
        config: Path::new(&config),
        vars: &[],
    };
    ctl::start(&run, &start).map_err(|e| {
        Stop::After(format!(
            "stopped, and the new doze did not start: {e}; doze up starts it"
        ))
    })?;
    let conn = client
        .dial()
        .map_err(|e| Stop::After(format!("started, and did not answer: {e}")))?;
    for name in &awake {
        if let Err(e) = conn.run_verb(Verb::Up, name) {
            return Ok(Woke::Partly(format!("waking {name} again failed: {e}")));
        }
    }
    Ok(Woke::All(awake))
}
