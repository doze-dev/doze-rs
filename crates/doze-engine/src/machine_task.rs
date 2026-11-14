//! The machine's task inputs (Go's `runTask`): a task's start is the machine's like any other (one at a time, its needs
//! held, cancelled by a stop), but it decides for itself whether it has to run, and it ends `done` rather than running.

use super::{Effect, Machine};
use crate::events::{Cause, Event, Kind};
use crate::lifecycle::Now;
use std::time::Duration;

impl Machine {
    /// `name` is a task.
    pub fn declare_task(&mut self, name: &str) {
        self.tasks.insert(name.into());
    }

    /// Start `boot` of task `name` has to run, for `why`: it is `booting` and its wake is journalled with why.
    pub fn task_runs(&mut self, name: &str, boot: u64, cause: Cause, why: &str) -> Vec<Effect> {
        if !self.current_boot(name, boot) || self.ctl(name).cancel_boot {
            return Vec::new();
        }
        self.table.mark_booting(name);
        vec![Effect::Journal(
            Event::new(Kind::Wake, name).cause(cause).detail(why),
        )]
    }

    /// Start `boot` of task `name` did not have to run (up to date, or manual and not asked for): nothing changes.
    pub fn task_skipped(&mut self, name: &str, boot: u64, now: Now) -> Vec<Effect> {
        if !self.current_boot(name, boot) {
            return Vec::new();
        }
        let c = self.ctl(name);
        c.boot = None;
        c.cancel_boot = false;
        let held = std::mem::take(&mut c.held);
        for h in &held {
            self.table.release(h, now);
        }
        Vec::new()
    }

    /// Task run `boot` of `name` was stopped while its command ran (a stop, `off`, the shutdown), and the command is
    /// gone: asleep as any cancelled start, with Go's `failed` "was stopped" in the journal.
    pub fn task_stopped(
        &mut self,
        name: &str,
        boot: u64,
        msg: &str,
        took: Duration,
        now: Now,
    ) -> Vec<Effect> {
        if !self.current_boot(name, boot) {
            return Vec::new();
        }
        let mut fx = vec![Effect::Journal(
            Event::new(Kind::Failed, name).took(took).detail(msg),
        )];
        fx.extend(self.boot_cancelled(name, boot, now));
        fx
    }

    /// Task run `boot` of `name` exited 0: the needs it held are released, it is `done`.
    pub fn task_done(&mut self, name: &str, boot: u64, took: Duration, now: Now) -> Vec<Effect> {
        if !self.current_boot(name, boot) {
            return Vec::new();
        }
        if self.ctl(name).cancel_boot {
            return self.boot_cancelled(name, boot, now);
        }
        let c = self.ctl(name);
        c.boot = None;
        let held = std::mem::take(&mut c.held);
        for h in &held {
            self.table.release(h, now);
        }
        self.table.mark_done(name);
        vec![Effect::Journal(
            Event::new(Kind::Ready, name).took(took).detail("exit 0"),
        )]
    }
}
