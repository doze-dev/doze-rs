//! The lifecycle of every instance of a stack, with no I/O: `reaped → booting → active ⇄ idle → reaped`, `crashing`,
//! `failed`, `done` (Go's `registry`, plus the exit decision that sat in `runtime.watch`). Time is passed in, so tests
//! drive it with synthetic clocks and arbitrary event sequences (`tests/lifecycle_props.rs`).
//!
//! The async shell ([`crate::Engine`]) holds a [`Table`] under a short lock, never across an await, and does what the
//! table decides.

use crate::godur::go_duration;
use crate::restart::{Restart, STABILITY_WINDOW, backoff_for};
use crate::state::{Instance, State};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant, SystemTime};

/// A moment, as both clocks see it: monotonic for every comparison, wall for what is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    pub mono: Instant,
    pub wall: SystemTime,
}

impl Now {
    /// This moment.
    pub fn current() -> Now {
        Now {
            mono: Instant::now(),
            wall: SystemTime::now(),
        }
    }

    /// `self` moved forward by `d` (for tests and deadlines).
    pub fn after(self, d: Duration) -> Now {
        Now {
            mono: self.mono + d,
            wall: self.wall + d,
        }
    }

    fn wall_of(self, t: Instant) -> SystemTime {
        self.wall
            .checked_sub(self.mono.saturating_duration_since(t))
            .unwrap_or(self.wall)
    }
}

/// How an exit by itself ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterExit {
    /// Start it again after `delay`; this is attempt `attempt` of `max`.
    Restart {
        delay: Duration,
        attempt: u32,
        max: u32,
    },
    /// The budget is spent: failed.
    GaveUp { max: u32 },
    /// Not restarted: failed, or (a supervised process that exited 0 under `on_failure`) reaped with a note.
    Stays,
}

/// How an instance stopped, for [`Table::exited`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit<'a> {
    /// Exited 0.
    pub success: bool,
    /// Go's words for it: "exited with status 1".
    pub text: &'a str,
    /// Its restart policy, if it has one.
    pub restart: Option<Restart>,
    /// A long-lived process the engine watches (an app), not a backend that wakes on a connection.
    pub supervised: bool,
    /// The stack is shutting down: nothing restarts.
    pub shutting_down: bool,
}

#[derive(Debug, Clone, Default)]
struct Slot {
    view: Instance,
    started: Option<Instant>,
    idle_since: Option<Instant>,
    next_restart: Option<Instant>,
}

/// What a subscriber of transitions compares (Go's `instSig`): connection churn alone is not a transition.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sig {
    state: State,
    healthy: Option<bool>,
    last_error: String,
    tainted: bool,
    restarts: u32,
    keep_awake: bool,
    off: bool,
}

/// Every instance's lifecycle. See the module docs.
#[derive(Debug, Default)]
pub struct Table {
    slots: BTreeMap<String, Slot>,
    version: u64,
    touched: BTreeSet<String>,
    last_sig: BTreeMap<String, Sig>,
}

impl Table {
    /// An empty table.
    pub fn new() -> Table {
        Table::default()
    }

    /// Bumped by every change, connection churn included (what a "something changed" watcher waits on).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The state of `name`, if it is known.
    pub fn state(&self, name: &str) -> Option<State> {
        self.slots.get(name).map(|s| s.view.state)
    }

    /// A copy of `name` as of `now`.
    pub fn get(&self, name: &str, now: Now) -> Option<Instance> {
        self.slots.get(name).map(|s| view_of(s, now))
    }

    /// Copies of every instance, sorted by name.
    pub fn snapshot(&self, now: Now) -> Vec<Instance> {
        self.slots.values().map(|s| view_of(s, now)).collect()
    }

    /// Instances whose observable state changed since the last call (lossy feed; Go's `emit` signature check).
    pub fn take_transitions(&mut self, now: Now) -> Vec<Instance> {
        let mut out = Vec::new();
        for name in std::mem::take(&mut self.touched) {
            let Some(slot) = self.slots.get(&name) else {
                continue;
            };
            let sig = sig_of(&slot.view);
            if self.last_sig.get(&name) != Some(&sig) {
                self.last_sig.insert(name, sig);
                out.push(view_of(slot, now));
            }
        }
        out
    }

    fn slot(&mut self, name: &str) -> &mut Slot {
        self.version = self.version.wrapping_add(1);
        self.touched.insert(name.to_string());
        self.slots.entry(name.to_string()).or_insert_with(|| Slot {
            view: Instance {
                name: name.to_string(),
                ..Instance::default()
            },
            ..Slot::default()
        })
    }

    /// Makes `name` known (reaped) without changing anything else.
    pub fn declare(&mut self, name: &str) {
        self.slot(name);
    }

    /// A start began; it clears the previous failure and what described an earlier process. Holds and the restart
    /// budget stay (Go: a restart that is booting still shows "try n of m").
    pub fn mark_booting(&mut self, name: &str) {
        let s = self.slot(name);
        s.view.backend.clear();
        s.view.pid = 0;
        s.view.healthy = None;
        s.started = None;
        s.idle_since = None;
        s.view.state = State::Booting;
        s.view.last_error.clear();
    }

    /// Records the latest failure without changing the state.
    pub fn set_error(&mut self, name: &str, msg: &str) {
        self.slot(name).view.last_error = msg.to_string();
    }

    /// Marks its declared structure known-incomplete (a convergence or setup failed), or clears that (only a
    /// successful convergence does).
    pub fn set_tainted(&mut self, name: &str, tainted: bool) {
        self.slot(name).view.tainted = tainted;
    }

    /// Records the last health probe.
    pub fn set_healthy(&mut self, name: &str, healthy: bool) {
        self.slot(name).view.healthy = Some(healthy);
    }

    /// How the reaper treats it: never, or after its own time (`None`: the stack's).
    pub fn set_idle_policy(&mut self, name: &str, always_on: bool, idle_after: Option<Duration>) {
        let s = self.slot(name);
        s.view.always_on = always_on;
        s.view.idle_after = idle_after;
    }

    /// Takes it out of this run (`off`) or puts it back.
    pub fn set_off(&mut self, name: &str, off: bool) {
        self.slot(name).view.off = off;
    }

    /// Whether it was taken out of this run.
    pub fn is_off(&self, name: &str) -> bool {
        self.slots.get(name).is_some_and(|s| s.view.off)
    }

    /// Flips the pin and returns the new value.
    pub fn toggle_pin(&mut self, name: &str) -> bool {
        let s = self.slot(name);
        s.view.keep_awake = !s.view.keep_awake;
        s.view.keep_awake
    }

    /// A start finished: running at `backend` as `pid`.
    pub fn mark_running(&mut self, name: &str, backend: &str, pid: u32, now: Now) {
        let s = self.slot(name);
        s.view.backend = backend.to_string();
        s.view.pid = pid;
        s.started = Some(now.mono);
        s.view.last_error.clear();
        s.view.restart_max = 0;
        s.next_restart = None;
        if s.view.conns > 0 || s.view.always_on {
            s.view.state = State::Active;
            s.idle_since = None;
        } else {
            s.view.state = State::Idle;
            s.idle_since = Some(now.mono);
        }
    }

    /// Nothing runs any more (stopped on purpose, or exited).
    pub fn mark_reaped(&mut self, name: &str) {
        let s = self.slot(name);
        clear_runtime(s);
        s.view.state = State::Reaped;
    }

    /// Nothing runs, and `msg` says what went wrong.
    pub fn mark_failed(&mut self, name: &str, msg: &str) {
        let s = self.slot(name);
        clear_runtime(s);
        s.view.state = State::Failed;
        s.view.last_error = msg.to_string();
    }

    /// A command that runs to completion exited 0.
    pub fn mark_done(&mut self, name: &str) {
        let s = self.slot(name);
        clear_runtime(s);
        s.view.state = State::Done;
        s.view.last_error.clear();
    }

    /// An intentional stop: the restart budget starts over.
    pub fn reset_restart(&mut self, name: &str) {
        self.slot(name).view.restart_count = 0;
    }

    /// One more hold (a client connection, or a running dependant). An idle instance becomes active.
    pub fn acquire(&mut self, name: &str) -> u32 {
        let s = self.slot(name);
        s.view.conns = s.view.conns.saturating_add(1);
        s.idle_since = None;
        if s.view.state == State::Idle {
            s.view.state = State::Active;
        }
        s.view.conns
    }

    /// One hold fewer. At zero the idle time starts, and an active instance that may sleep becomes idle.
    pub fn release(&mut self, name: &str, now: Now) -> u32 {
        let s = self.slot(name);
        s.view.conns = s.view.conns.saturating_sub(1);
        if s.view.conns == 0 {
            s.idle_since = Some(now.mono);
            if s.view.state == State::Active && !s.view.always_on {
                s.view.state = State::Idle;
            }
        }
        s.view.conns
    }

    /// The instances the reaper stops now: idle, held by nothing, not pinned, for at least their own idle time or
    /// else `default` (none: never). Sorted.
    pub fn reapable(&self, default: Option<Duration>, now: Now) -> Vec<String> {
        let mut out = Vec::new();
        for (name, s) in &self.slots {
            let Some(limit) = s.view.idle_after.filter(|d| !d.is_zero()).or(default) else {
                continue;
            };
            if s.view.keep_awake
                || limit.is_zero()
                || s.view.state != State::Idle
                || s.view.conns != 0
            {
                continue;
            }
            if s.idle_since
                .is_some_and(|t| now.mono.saturating_duration_since(t) >= limit)
            {
                out.push(name.clone());
            }
        }
        out
    }

    /// It exited by itself: reaped, then restarted, given up on, or failed, as Go's crash watcher decided.
    pub fn exited(&mut self, name: &str, exit: Exit<'_>, now: Now) -> AfterExit {
        let uptime = self
            .slots
            .get(name)
            .and_then(|s| s.started)
            .map(|t| now.mono.saturating_duration_since(t));
        self.mark_reaped(name);
        if exit.shutting_down {
            return AfterExit::Stays;
        }
        if let Some(r) = exit.restart.filter(|r| r.active() && r.wants(exit.success)) {
            return self.crashed(name, exit.text, r, uptime, now);
        }
        if exit.supervised {
            if exit.success {
                self.set_error(name, "exited with status 0 and was not restarted");
            } else {
                self.mark_failed(name, &format!("{}; not restarted", exit.text));
            }
        } else {
            self.mark_failed(
                name,
                "exited unexpectedly; it wakes again on the next connection",
            );
        }
        AfterExit::Stays
    }

    fn crashed(
        &mut self,
        name: &str,
        text: &str,
        r: Restart,
        uptime: Option<Duration>,
        now: Now,
    ) -> AfterExit {
        let s = self.slot(name);
        if uptime.is_some_and(|u| u >= STABILITY_WINDOW) {
            s.view.restart_count = 0;
        }
        s.view.restart_count = s.view.restart_count.saturating_add(1);
        let attempt = s.view.restart_count;
        if attempt > r.max_retries {
            let msg = format!("{text}; gave up after {} restarts", r.max_retries);
            self.mark_failed(name, &msg);
            return AfterExit::GaveUp { max: r.max_retries };
        }
        let delay = backoff_for(r.backoff, attempt);
        s.view.state = State::Crashing;
        s.view.restart_max = r.max_retries;
        s.next_restart = Some(now.mono + delay);
        s.view.last_error = format!(
            "{text}; restarting ({attempt}/{}) in {}",
            r.max_retries,
            go_duration(delay)
        );
        AfterExit::Restart {
            delay,
            attempt,
            max: r.max_retries,
        }
    }
}

/// What describes a running process goes. Holds stay: whoever holds it (a running dependant, an open client) still
/// does, and releases it later; Go zeroed them here, so those releases under-counted the next start.
fn clear_runtime(s: &mut Slot) {
    s.view.backend.clear();
    s.view.pid = 0;
    s.started = None;
    s.idle_since = None;
    s.view.healthy = None;
    s.view.restart_max = 0;
    s.next_restart = None;
}

fn view_of(s: &Slot, now: Now) -> Instance {
    let mut v = s.view.clone();
    v.started_at = s.started.map(|t| now.wall_of(t));
    v.idle_since = s.idle_since.map(|t| now.wall_of(t));
    v.next_restart = s.next_restart.map(|t| now.wall_of(t));
    v
}

fn sig_of(v: &Instance) -> Sig {
    Sig {
        state: v.state,
        healthy: v.healthy,
        last_error: v.last_error.clone(),
        tainted: v.tainted,
        restarts: v.restart_count,
        keep_awake: v.keep_awake,
        off: v.off,
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
