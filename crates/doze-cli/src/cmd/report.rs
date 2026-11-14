//! What a verb did to each service, said as it happens: `✓ db awake in 38ms`, one line each on stdout (Go's
//! `verbReport`), and kept for `--json` as `{"results": […]}` in the order they ran. A verb that fails stops there:
//! the services before it keep their lines, the failure is the run's error.

use crate::diag::Diagnostic;
use crate::output::Mode;
use crate::paint::Paint;
use crate::stack::daemon_error;
use serde::Serialize;
use std::io::Write;
use std::time::Duration;

/// What a verb did to one service.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Outcome {
    pub(crate) name: String,
    /// What it is now: awake, asleep, restarted, reset, pinned, unpinned, on, off, done.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) state: String,
    /// How long it took to become ready, by the daemon's journal.
    #[serde(skip_serializing_if = "is_zero")]
    pub(crate) took_ms: u64,
    /// Whether this run changed anything, for a verb that can know (pin).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) changed: Option<bool>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) error: String,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // WHY: serde's skip_serializing_if passes a reference
fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// The outcomes so far, printed as they come.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub(crate) results: Vec<Outcome>,
    #[serde(skip)]
    print: bool,
    #[serde(skip)]
    paint: Option<Paint>,
}

impl Report {
    /// A report that prints unless the run is `--json` or `-q`.
    pub(crate) fn new(mode: &Mode) -> Report {
        Report {
            results: Vec::new(),
            print: !mode.json && !mode.quiet,
            paint: Some(mode.out()),
        }
    }

    fn p(&self) -> Paint {
        self.paint.unwrap_or(Paint::new(false))
    }

    /// Records `name` as `state` and prints `✓ <name> <line>`, with how long it took when the journal said.
    pub(crate) fn ok(
        &mut self,
        out: &mut dyn Write,
        name: &str,
        state: &str,
        line: &str,
        took: Duration,
    ) {
        self.results.push(Outcome {
            name: name.to_owned(),
            state: state.to_owned(),
            took_ms: u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
            ..Outcome::default()
        });
        let suffix = if took.is_zero() {
            String::new()
        } else {
            self.p().dim(&format!(" in {}", round(took))).into_owned()
        };
        self.say(out, &format!("{} {name} {line}{suffix}", self.p().ok("✓")));
    }

    /// Records a pin state and whether this run changed it.
    pub(crate) fn pin(&mut self, out: &mut dyn Write, name: &str, pinned: bool, changed: bool) {
        let (state, line) = if pinned {
            (
                "pinned",
                "pinned — it stays awake until unpinned or the daemon stops",
            )
        } else {
            ("unpinned", "unpinned — it sleeps again when idle")
        };
        let line = if changed {
            line.to_owned()
        } else {
            format!("already {state}")
        };
        self.results.push(Outcome {
            name: name.to_owned(),
            state: state.to_owned(),
            changed: Some(changed),
            ..Outcome::default()
        });
        self.say(out, &format!("{} {name} {line}", self.p().ok("✓")));
    }

    /// Records a result without a line (what `sleep` with no name put to sleep).
    pub(crate) fn quiet(&mut self, name: &str, state: &str) {
        self.results.push(Outcome {
            name: name.to_owned(),
            state: state.to_owned(),
            ..Outcome::default()
        });
    }

    /// Prints a line that is not one service's (`✓ everything is asleep`).
    pub(crate) fn say(&self, out: &mut dyn Write, line: &str) {
        if self.print {
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
        }
    }

    /// Records why `name` failed and gives the run's error: the daemon's words, and where to look.
    pub(crate) fn fail(&mut self, name: &str, e: &doze_control::Error) -> Diagnostic {
        let d = daemon_error(e, Some(name));
        self.results.push(Outcome {
            name: name.to_owned(),
            error: e.to_string(),
            ..Outcome::default()
        });
        if d.fix.is_some() {
            d
        } else {
            d.fix(format!("doze logs {name}"))
        }
    }
}

/// A duration as a person says it (Go `roundDuration`): 38ms, 1.2s, 2m5s.
pub(crate) fn round(d: Duration) -> String {
    let to = if d < Duration::from_secs(1) {
        Duration::from_millis(1)
    } else if d < Duration::from_secs(60) {
        Duration::from_millis(100)
    } else {
        Duration::from_secs(1)
    };
    doze_control::format_go_duration(go_round(d, to))
}

/// Go's `Duration.Round`: to the nearest multiple of `m`, halves away from zero.
fn go_round(d: Duration, m: Duration) -> Duration {
    let (d, m) = (d.as_nanos(), m.as_nanos().max(1));
    let r = d % m;
    let down = d - r;
    let n = if r + r < m { down } else { down + m };
    Duration::from_nanos(u64::try_from(n).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_go_says_them() {
        assert_eq!(round(Duration::from_millis(38)), "38ms");
        assert_eq!(round(Duration::from_millis(1240)), "1.2s");
        assert_eq!(round(Duration::from_millis(1250)), "1.3s");
        assert_eq!(round(Duration::from_secs(125)), "2m5s");
        assert_eq!(round(Duration::from_micros(310_400)), "310ms");
    }
}
