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

/// Moves every running stack under `home` to the doze at `exe`.
pub(crate) fn all(home: &Path, exe: &Path) -> Vec<Moved> {
    doze_projects::running(home)
        .into_iter()
        .map(|r| {
            let stack = doze_projects::load(r.key.dir())
                .ok()
                .flatten()
                .map(|p| p.name)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| r.key.slug());
            match one(&r.key, exe) {
                Ok(woken) => Moved {
                    stack,
                    moved: true,
                    detail: if woken.is_empty() {
                        "nothing was awake".into()
                    } else {
                        format!("woken again: {}", woken.join(", "))
                    },
                },
                Err(why) => Moved {
                    stack,
                    moved: false,
                    detail: why,
                },
            }
        })
        .collect()
}

/// Stops the stack's daemon and starts it again from `exe`; what it woke again.
fn one(key: &StackKey, exe: &Path) -> Result<Vec<String>, String> {
    let config = doze_projects::load(key.dir())
        .ok()
        .flatten()
        .map(|p| p.config)
        .filter(|c| !c.is_empty())
        .ok_or(
            "its record names no doze.hcl to start it with; doze down, then doze up, moves it",
        )?;
    let run = RunDir::of(key.dir());
    let _lock = StackLock::take(&run).map_err(|e| format!("could not take its lock: {e}"))?;
    let client = Client::socket(key.socket())
        .named("cli")
        .with_timeout(Some(ASK));
    let snap = client
        .dial()
        .and_then(|c| c.snapshot())
        .map_err(|e| format!("its daemon did not answer: {e}"))?;
    let awake: Vec<String> = snap
        .instances
        .iter()
        .filter(|v| v.declared && crate::cmd::status::awake(&v.state))
        .map(|v| v.name.clone())
        .collect();
    let pid = down::identify(key)
        .ok_or("its pid file names no daemon doze started; it was left running")?;
    if !down::stop(pid).map_err(|d| d.message)? {
        return Err("its daemon is another user's; it was left running".into());
    }
    down::clear(&down::leftovers(key));
    let start = ctl::Start {
        exe,
        config: Path::new(&config),
        vars: &[],
    };
    ctl::start(&run, &start)
        .map_err(|e| format!("stopped, and the new doze did not start: {e}; doze up starts it"))?;
    let conn = client
        .dial()
        .map_err(|e| format!("started, and did not answer: {e}"))?;
    for name in &awake {
        conn.run_verb(Verb::Up, name)
            .map_err(|e| format!("started; waking {name} again failed: {e}"))?;
    }
    Ok(awake)
}
