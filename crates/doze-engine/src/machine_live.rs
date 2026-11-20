//! Instances added to and removed from a running stack (Go's `AddInstance`, `RemoveInstance`): the machine learns a
//! name, or forgets one that is stopped and that nothing holds.

use super::Machine;
use crate::error::Error;

impl Machine {
    /// Makes `name` known: reaped, nothing held, nothing in flight; `task` when it is a task.
    pub fn declare(&mut self, name: &str, task: bool) {
        self.declared.insert(name.into());
        self.generations += 1;
        self.generation.insert(name.into(), self.generations);
        if task {
            self.tasks.insert(name.into());
        }
        self.table.declare(name);
    }

    /// Forgets `name`. Refused while it runs, is being started or stopped, or holds or is held by another instance:
    /// the caller stops it (and takes it out of the run, so nothing starts it again) first.
    pub fn forget(&mut self, name: &str) -> Result<(), Error> {
        self.declared(name)?;
        let busy = self.ctl.get(name).is_some_and(|c| {
            c.unit.is_some() || c.boot.is_some() || !c.stopping.is_empty() || !c.held.is_empty()
        });
        let held = self.ctl.values().any(|c| {
            c.held.iter().any(|h| h == name)
                || c.stopping
                    .values()
                    .any(|(_, h)| h.iter().any(|n| n == name))
        });
        if busy || held || self.table.state(name).is_some_and(|s| s.running()) {
            return Err(Error::Failed(format!(
                "instance \"{name}\" is still running; stop it before removing it"
            )));
        }
        self.declared.remove(name);
        self.generation.remove(name);
        self.tasks.remove(name);
        self.ctl.remove(name);
        self.table.forget(name);
        Ok(())
    }
}
