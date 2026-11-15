//! Verbs and module actions, each on a short-lived connection of its own, so a long verb never holds up the stream
//! (PLAN.md §4.3). They block the caller: the UI calls them from a background task.

use super::Hub;
use crate::error::Error;
use crate::stack::lock;
use doze_control::{Client, Verb};
use doze_projects::StackKey;
use std::time::Duration;

/// How long a verb or an action may take, as the Go hub allowed.
pub const VERB_TIMEOUT: Duration = Duration::from_secs(30);

impl Hub {
    /// Runs a verb on one running stack and waits for it. While the daemon is not answering it refuses at once, as
    /// the dash did: dialling would hang and then say something worse (Go `Do`).
    pub fn verb(&self, key: &StackKey, verb: Verb, target: &str) -> Result<(), Error> {
        self.live_client(key)?.dial()?.run_verb(verb, target)?;
        Ok(())
    }

    /// Runs a module's action on an instance (cancel a query, produce a message) and returns what it said (Go
    /// `Admin`).
    pub fn admin(
        &self,
        key: &StackKey,
        inst: &str,
        action: &str,
        resource: &str,
        input: &str,
    ) -> Result<String, Error> {
        Ok(self
            .live_client(key)?
            .dial()?
            .admin(inst, action, resource, input)?)
    }

    fn live_client(&self, key: &StackKey) -> Result<Client, Error> {
        let inner = lock(&self.shared.inner);
        let s = inner.stacks.get(key).ok_or(Error::NotRunning)?;
        if !s.is_live() {
            return Err(Error::NotResponding);
        }
        Ok(s.client().clone().with_timeout(Some(VERB_TIMEOUT)))
    }
}
