//! What a fold changed, so the UI redraws only what it must: a [`Changed`] per fold, merged into a [`Batch`] per frame.

use doze_projects::StackKey;
use std::collections::BTreeMap;
use std::fmt;
use std::ops::{BitOr, BitOrAssign};

/// Which parts of a stack changed: a small set of flags.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Parts(u16);

impl Parts {
    /// Nothing.
    pub const NONE: Parts = Parts(0);
    /// The list of stacks: one appeared, went away, stopped or started (on [`Changed::key`] `None`).
    pub const LIST: Parts = Parts(1);
    /// The stack's name, folder, idle time, when it loaded, what it started last time.
    pub const META: Parts = Parts(1 << 1);
    /// The connection: connecting, live, down, the retry time.
    pub const CONN: Parts = Parts(1 << 2);
    /// The instances.
    pub const INSTANCES: Parts = Parts(1 << 3);
    /// The journal.
    pub const JOURNAL: Parts = Parts(1 << 4);
    /// The meters.
    pub const STATS: Parts = Parts(1 << 5);
    /// The log ring.
    pub const LOGS: Parts = Parts(1 << 6);
    /// A module console.
    pub const CONSOLE: Parts = Parts(1 << 7);

    const NAMES: [(Parts, &'static str); 8] = [
        (Parts::LIST, "list"),
        (Parts::META, "meta"),
        (Parts::CONN, "conn"),
        (Parts::INSTANCES, "instances"),
        (Parts::JOURNAL, "journal"),
        (Parts::STATS, "stats"),
        (Parts::LOGS, "logs"),
        (Parts::CONSOLE, "console"),
    ];

    /// Whether every flag of `other` is set.
    pub fn contains(self, other: Parts) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any flag of `other` is set.
    pub fn intersects(self, other: Parts) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether no flag is set.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for Parts {
    type Output = Parts;
    fn bitor(self, rhs: Parts) -> Parts {
        Parts(self.0 | rhs.0)
    }
}

impl BitOrAssign for Parts {
    fn bitor_assign(&mut self, rhs: Parts) {
        self.0 |= rhs.0;
    }
}

impl fmt::Debug for Parts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = Parts::NAMES
            .iter()
            .filter(|(p, _)| self.contains(*p))
            .map(|(_, n)| *n)
            .collect();
        write!(f, "Parts({})", names.join("|"))
    }
}

/// What one fold changed: parts of one stack, or (`key` `None`) the list of stacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub key: Option<StackKey>,
    pub parts: Parts,
}

impl Changed {
    /// Parts of one stack.
    pub fn stack(key: &StackKey, parts: Parts) -> Changed {
        Changed {
            key: Some(key.clone()),
            parts,
        }
    }

    /// The list of stacks.
    pub fn list() -> Changed {
        Changed {
            key: None,
            parts: Parts::LIST,
        }
    }
}

/// Every change since the UI last drew, merged: what the list did, and the parts of each stack.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batch {
    /// What changed about the hub as a whole ([`Parts::LIST`]).
    pub hub: Parts,
    /// What changed in each stack.
    pub stacks: BTreeMap<StackKey, Parts>,
}

impl Batch {
    /// Folds one change in.
    pub fn add(&mut self, c: Changed) {
        if c.parts.is_empty() {
            return;
        }
        match c.key {
            None => self.hub |= c.parts,
            Some(key) => *self.stacks.entry(key).or_default() |= c.parts,
        }
    }

    /// Folds another batch in.
    pub fn merge(&mut self, other: Batch) {
        self.hub |= other.hub;
        for (key, parts) in other.stacks {
            *self.stacks.entry(key).or_default() |= parts;
        }
    }

    /// What changed in the stack `key`.
    pub fn parts(&self, key: &StackKey) -> Parts {
        self.stacks.get(key).copied().unwrap_or_default()
    }

    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.hub.is_empty() && self.stacks.is_empty()
    }
}
