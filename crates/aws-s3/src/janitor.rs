//! The lifecycle janitor. It costs nothing until a bucket has an enabled lifecycle rule: no task exists, no timer is armed, and the
//! service can be reaped for idleness. Putting a rule arms it (and leaves a marker file in the region's directory, so a restart
//! knows without opening any database); it sleeps to the next UTC midnight, when rules take effect, asks each armed region to
//! sweep, and stops when no region has a rule left.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use doze_fs::fs::{Fs, RealFs};
use doze_runtime::{Clock, Tasks};

use crate::HOME_REGION;

/// The file that says a region has lifecycle rules.
pub(crate) const MARKER: &str = "lifecycle.armed";
const DAY_MS: i64 = 86_400_000;

/// Which regions the janitor visits, and whether its task is running.
#[derive(Debug, Default)]
pub(crate) struct Janitor {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    regions: BTreeSet<String>,
    running: bool,
}

/// The regions that left a marker in `base`.
pub(crate) fn armed_regions(base: &Path) -> Vec<String> {
    let fs = RealFs;
    let mut found = Vec::new();
    if fs.open_existing(&base.join(MARKER)).is_ok() {
        found.push(HOME_REGION.to_owned());
    }
    for e in fs.list_dir(&base.join("regions")).unwrap_or_default() {
        if e.is_dir && fs.open_existing(&base.join("regions").join(&e.name).join(MARKER)).is_ok() {
            found.push(e.name);
        }
    }
    found
}

/// Milliseconds from `now_ms` to the next UTC midnight.
pub(crate) fn until_midnight(now_ms: i64) -> i64 {
    DAY_MS - now_ms.rem_euclid(DAY_MS)
}

impl Janitor {
    /// Adds `region` to those visited. Returns true when the task needs starting.
    pub(crate) fn arm(&self, region: &str) -> bool {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        s.regions.insert(region.to_owned());
        !std::mem::replace(&mut s.running, true)
    }

    pub(crate) fn regions(&self) -> Vec<String> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).regions.iter().cloned().collect()
    }

    /// Drops a region with no rules left; true when none remain (the task ends).
    pub(crate) fn disarm_all(&self) {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        s.regions.clear();
        s.running = false;
    }

    fn disarm(&self, region: &str) -> bool {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        s.regions.remove(region);
        if s.regions.is_empty() {
            s.running = false;
        }
        s.regions.is_empty()
    }
}

/// Runs the janitor's loop: sleep to midnight, sweep each armed region, repeat until none is armed or the service stops.
pub(crate) async fn run(inner: Arc<crate::Inner>, tasks: Tasks, clock: Arc<dyn Clock>) {
    let stopping = tasks.stopping();
    loop {
        let wait = Duration::from_millis(u64::try_from(until_midnight(clock.now_ms())).unwrap_or(1000));
        if stopping.run_until_cancelled(tokio::time::sleep(wait)).await.is_none() {
            return;
        }
        for region in inner.janitor.regions() {
            let armed = inner.sweep_now(&region).await.map_or(true, |s| s.armed);
            if !armed && inner.janitor.disarm(&region) {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_janitor_sleeps_to_the_next_midnight() {
        assert_eq!(until_midnight(0), DAY_MS);
        assert_eq!(until_midnight(DAY_MS - 1), 1);
        assert_eq!(until_midnight(5 * DAY_MS + 3_600_000), DAY_MS - 3_600_000);
    }

    #[test]
    fn it_runs_once_however_many_regions_arm_it() {
        let j = Janitor::default();
        assert!(j.arm("us-east-1"), "the first arming starts the task");
        assert!(!j.arm("eu-west-1"), "a second does not start another");
        assert!(!j.disarm("us-east-1"));
        assert!(j.disarm("eu-west-1"), "the last to go ends it");
        assert!(j.arm("us-east-1"), "and a later rule starts it again");
    }
}
