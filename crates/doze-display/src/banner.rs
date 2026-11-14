//! What a surface says about its daemon, and about a task the person asked for. A screen that goes on showing the
//! last picture of a daemon that has gone is the worst thing a dashboard can do: it looks healthy and is not. So
//! every page says when what it shows cannot be believed, in these words.

use std::time::Duration;

use crate::gotime::{self, SECOND};
use crate::model::TaskRecord;
use crate::note::Said;
use crate::words::took;

/// How long without a frame before a page says its picture is old. The daemon sends the meters every second, so
/// three is already late.
pub const STALE_AFTER: Duration = Duration::from_secs(3);

/// What to put at the top of a page in place of the resident total when what it shows cannot be believed; `None`
/// when it can. `connecting` and `down` are the connection's state; `retry_in` how long until the next attempt
/// while down; `since_frame` how long since the last frame arrived, or zero when none has. (Go: `Banner`.)
#[must_use]
pub fn banner(
    connecting: bool,
    down: bool,
    retry_in: Duration,
    since_frame: Duration,
) -> Option<String> {
    if connecting {
        return Some("connecting to the daemon…".to_owned());
    }
    if down {
        let raw = gotime::nanos(retry_in);
        return Some(if gotime::round(raw, SECOND) > 0 {
            format!(
                "daemon not answering · retrying in {}",
                crate::grain::seconds_left(raw)
            )
        } else {
            "daemon not answering · retrying".to_owned()
        });
    }
    // Coarse (a window looked at, not used): "last update <1m ago", then Go's `Ago` minutes.
    (since_frame > STALE_AFTER).then(|| {
        let d = gotime::nanos(since_frame);
        if crate::grain::is_coarse() {
            format!("last update {} ago", crate::format::ago_nanos(d))
        } else {
            format!("last update {}s ago", gotime::whole_seconds(d))
        }
    })
}

/// A connection error as one short sentence. (Go: `Humanise`, given the error's text.)
#[must_use]
pub fn humanise(err: &str) -> &str {
    let has = |words: &[&str]| words.iter().any(|w| err.contains(w));
    if has(&["connection refused", "no such file", "not reachable"]) {
        "the daemon is not reachable"
    } else if has(&["EOF", "broken pipe", "connection reset"]) {
        "the daemon closed the connection"
    } else {
        err
    }
}

/// The header's count of tasks running: "◌ 2 running", or `None` when none is. (Go: `RunningNote`.)
#[must_use]
pub fn running_note(n: usize) -> Option<String> {
    (n > 0).then(|| format!("◌ {n} running"))
}

/// Said at once when a task is asked for: the daemon runs it before it answers, so for a while nothing else on the
/// screen would say so. (Go: `TaskStarted`.)
#[must_use]
pub fn task_started(target: &str) -> String {
    format!("running {target}…")
}

/// How a task that has just finished went, from the record the snapshot carries. (Go: `TaskResult`.)
#[must_use]
pub fn task_result(name: &str, t: &TaskRecord) -> Said {
    let took = took(t, 100 * gotime::MILLISECOND);
    if t.failed() {
        Said {
            text: format!("{name} failed (exit {}) after {took}", t.exit_code),
            bad: true,
        }
    } else {
        Said {
            text: format!("{name} done in {took}"),
            bad: false,
        }
    }
}
