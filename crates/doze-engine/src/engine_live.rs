//! Instances added to and removed from a running engine (Go's `AddInstance`, `RemoveInstance`): the stack is swapped
//! whole, checked as it was at build time, and the machine learns or forgets the name under the same lock.

use super::Engine;
use crate::error::{BuildError, Error};
use crate::events::Cause;
use crate::stack::{Kind, Service, Stack};
use std::sync::{Arc, PoisonError};

impl Engine {
    /// Adds `svc` to the running stack: its name new, its needs declared, no cycle. Nothing starts.
    pub fn add(&self, svc: Service) -> Result<(), BuildError> {
        self.update(|core| {
            let mut slot = self
                .inner
                .stack
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            let mut next = Stack::clone(&slot);
            let (name, task) = (svc.name.clone(), matches!(svc.kind, Kind::Task(_)));
            next.services.push(svc);
            crate::graph::check(&next)?;
            core.m.declare(&name, task);
            *slot = Arc::new(next);
            Ok(())
        })
    }

    /// Removes `name` from the running stack: refused while another instance needs it; otherwise it is taken out of
    /// the run (stopped, and nothing starts it again) and forgotten. Its data, logs and pid records stay (the caller
    /// wipes what it was asked to).
    pub async fn remove(&self, name: &str) -> Result<(), Error> {
        self.service(name)?;
        let needed: Vec<String> = self
            .stack()
            .services
            .iter()
            .filter(|s| s.needs.iter().any(|n| n.name == name))
            .map(|s| s.name.clone())
            .collect();
        if !needed.is_empty() {
            return Err(Error::Failed(format!(
                "instance \"{name}\" is needed by {}; remove {} first",
                quoted(&needed),
                if needed.len() == 1 { "it" } else { "them" }
            )));
        }
        self.set_off(name, true).await?;
        self.stop(name, Cause::Command).await?;
        self.update(|core| {
            let mut slot = self
                .inner
                .stack
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(s) = slot
                .services
                .iter()
                .find(|s| s.needs.iter().any(|n| n.name == name))
            {
                return Err(Error::Failed(format!(
                    "instance \"{name}\" is needed by \"{}\"; remove it first",
                    s.name
                )));
            }
            core.m.forget(name)?;
            core.live.remove(name);
            let mut next = Stack::clone(&slot);
            next.services.retain(|s| s.name != name);
            *slot = Arc::new(next);
            Ok(())
        })
    }
}

fn quoted(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(", ")
}
