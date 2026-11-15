//! What pages watch. Nothing streams for a page nobody is looking at (Go D14): a page holds a [`Watch`] while it
//! shows a stack's meters or logs, a [`ConsoleWatch`] while it shows a module console. Interest is kept by stack key,
//! so a stack that is not running yet, or whose daemon restarts, streams it as soon as it is there.

use super::refresh::wanted;
use super::{Hub, Shared};
use crate::stack::{Cmd, lock};
use doze_control::Topic;
use doze_projects::StackKey;
use std::sync::{Arc, Weak};

/// A page watching a topic of a stack. Dropping it says the page stopped.
#[derive(Debug)]
#[must_use = "the watch ends when it is dropped"]
pub struct Watch {
    hub: Weak<Shared>,
    key: StackKey,
    topic: Topic,
}

/// A page showing an instance's console. Dropping it says the page stopped.
#[derive(Debug)]
#[must_use = "the watch ends when it is dropped"]
pub struct ConsoleWatch {
    hub: Weak<Shared>,
    key: StackKey,
    inst: String,
}

impl Hub {
    /// A page shows a stack's meters ([`Topic::Stats`]) or logs ([`Topic::Logs`]), so the hub streams them. The
    /// subscription is remade when the first page arrives or the last leaves. Other topics are always streamed.
    pub fn watch(&self, key: &StackKey, topic: Topic) -> Watch {
        let mut inner = lock(&self.shared.inner);
        let n = inner
            .wants
            .entry(key.clone())
            .or_default()
            .entry(topic)
            .or_insert(0);
        *n += 1;
        if *n == 1 {
            let w = wanted(&inner, key);
            if let Some(s) = inner.stacks.get(key) {
                s.command(Cmd::Topics(w));
            }
        }
        Watch {
            hub: Arc::downgrade(&self.shared),
            key: key.clone(),
            topic,
        }
    }

    /// A page shows an instance's module console, so the hub subscribes to it.
    pub fn watch_console(&self, key: &StackKey, inst: &str) -> ConsoleWatch {
        let mut inner = lock(&self.shared.inner);
        let n = inner
            .consoles
            .entry(key.clone())
            .or_default()
            .entry(inst.to_owned())
            .or_insert(0);
        *n += 1;
        if *n == 1
            && let Some(s) = inner.stacks.get(key)
        {
            s.command(Cmd::Console {
                inst: inst.to_owned(),
                shown: true,
            });
        }
        ConsoleWatch {
            hub: Arc::downgrade(&self.shared),
            key: key.clone(),
            inst: inst.to_owned(),
        }
    }

    /// Makes a shown console follow a stream panel and the row of its table that scopes it.
    pub fn follow_console(&self, key: &StackKey, inst: &str, panel: &str, sector: &str) {
        let inner = lock(&self.shared.inner);
        if let Some(s) = inner.stacks.get(key) {
            s.command(Cmd::Follow {
                inst: inst.to_owned(),
                panel: panel.to_owned(),
                sector: sector.to_owned(),
            });
        }
    }

    /// How many pages watch `topic` of `key`.
    pub fn watchers(&self, key: &StackKey, topic: Topic) -> usize {
        let inner = lock(&self.shared.inner);
        inner
            .wants
            .get(key)
            .and_then(|m| m.get(&topic))
            .copied()
            .unwrap_or(0)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let Some(shared) = self.hub.upgrade() else {
            return;
        };
        let mut inner = lock(&shared.inner);
        let Some(n) = inner
            .wants
            .get_mut(&self.key)
            .and_then(|m| m.get_mut(&self.topic))
        else {
            return;
        };
        *n = n.saturating_sub(1);
        if *n == 0 {
            let w = wanted(&inner, &self.key);
            if let Some(s) = inner.stacks.get(&self.key) {
                s.command(Cmd::Topics(w));
            }
        }
    }
}

impl Drop for ConsoleWatch {
    fn drop(&mut self) {
        let Some(shared) = self.hub.upgrade() else {
            return;
        };
        let mut inner = lock(&shared.inner);
        let Some(m) = inner.consoles.get_mut(&self.key) else {
            return;
        };
        let still = m.get_mut(&self.inst).map_or(0, |n| {
            *n = n.saturating_sub(1);
            *n
        });
        if still > 0 {
            return;
        }
        m.remove(&self.inst);
        if let Some(s) = inner.stacks.get(&self.key) {
            s.command(Cmd::Console {
                inst: self.inst.clone(),
                shown: false,
            });
        }
    }
}
