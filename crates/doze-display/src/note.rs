//! The sentence beside a row: why it is awake, what is wrong, when it sleeps.

use std::time::{Duration, SystemTime};

use crate::gotime;
use crate::model::{Holder, Instance, State};

/// A sentence, and whether it reports a problem and should be drawn as one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    /// The words.
    pub text: String,
    /// It is a problem.
    pub bad: bool,
}

/// `s` up to its first newline. (Go: `FirstLine`.)
#[must_use]
pub fn first_line(s: &str) -> &str {
    s.split_once('\n').map_or(s, |(line, _)| line)
}

/// The note beside an instance's row. `idle` is the stack's idle time, which an instance's own overrides.
/// (Go: `Note`.)
#[must_use]
pub fn note(v: &Instance, idle: Duration, now: SystemTime) -> Said {
    if matches!(v.state, State::Crashing | State::Failed) {
        return Said {
            text: first_line(&v.last_error).to_owned(),
            bad: true,
        };
    }
    if v.tainted {
        let mut text = "its last sync did not finish".to_owned();
        if !v.last_error.is_empty() {
            text.push_str(": ");
            text.push_str(first_line(&v.last_error));
        }
        return Said { text, bad: true };
    }
    if !v.last_error.is_empty() && !v.state.awake() {
        return Said {
            text: first_line(&v.last_error).to_owned(),
            bad: false,
        };
    }
    let mut parts: Vec<String> = Vec::new();
    if !v.name_held.is_empty() {
        parts.push(v.name_held.clone());
    }
    let by: Vec<&str> = v.holders.iter().filter_map(dependant).collect();
    // the last connection holder's count, as the daemon sends one
    let clients = v.holders.iter().rev().find_map(|h| {
        if let Holder::Connections(n) = h {
            Some(*n)
        } else {
            None
        }
    });
    if !by.is_empty() {
        parts.push(format!("held by {}", by.join(", ")));
    }
    if v.keep_awake {
        parts.push("pinned".to_owned());
    }
    if v.name_only {
        parts.push("by name only: 127.0.0.1 has this port taken".to_owned());
    }
    let idle = gotime::nanos(if v.idle_after.is_zero() {
        idle
    } else {
        v.idle_after
    });
    let quiet = parts.is_empty() && clients.unwrap_or(0) == 0 && v.state == State::Idle && idle > 0;
    if quiet && v.idle_since.is_some() {
        let left = idle.wrapping_sub(gotime::since(now, v.idle_since));
        if left > 0 {
            parts.push(format!(
                "sleeps in {}",
                gotime::duration_string(gotime::round(left, gotime::SECOND))
            ));
        }
    }
    Said {
        text: parts.join(" · "),
        bad: false,
    }
}

pub(crate) fn dependant(h: &Holder) -> Option<&str> {
    match h {
        Holder::Dependant(name) => Some(name),
        _ => None,
    }
}
