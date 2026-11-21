//! Finding stacks: running ones by their control sockets, stopped ones by their project records (Go `Refresh`).

use super::{Hub, Inner};
use crate::changed::{Changed, Parts};
use crate::stack::{Stack, Wanted, lock};
use doze_control::Topic;
use doze_projects::{Project, StackKey};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

impl Hub {
    /// Looks once for stacks that appeared or went away. [`Hub::start`] calls it on its interval; a verb's caller
    /// calls it after, so the list is right at once.
    pub fn refresh(&self) {
        let home = self.shared.o.home.clone();
        // A socket that outlived its daemon (it was killed, or the machine restarted) is left out: the stack is
        // stopped, not "not answering".
        let running = doze_projects::running(&home);
        let records = doze_projects::list(&home);
        let seen: BTreeSet<StackKey> = running.iter().map(|r| r.key.clone()).collect();
        let mut changes = Vec::new();
        let mut list = false;
        let mut inner = lock(&self.shared.inner);
        for r in running {
            // Another pid on the socket is a new daemon (a restart quicker than a look): it is followed afresh, so
            // its connection starts at Connecting, not at the last one's Down.
            let replaced = inner
                .stacks
                .get(&r.key)
                .is_some_and(|s| matches!((s.pid, r.pid), (Some(a), Some(b)) if a != b));
            if replaced || !inner.stacks.contains_key(&r.key) {
                let mut stack = self.spawn(&inner, &r.key, &r.socket);
                stack.pid = r.pid;
                inner.stacks.insert(r.key.clone(), stack);
                if replaced {
                    changes.push(Changed::stack(&r.key, Parts::CONN | Parts::META));
                } else {
                    list = true;
                }
            } else if let Some(s) = inner.stacks.get_mut(&r.key) {
                s.pid = s.pid.or(r.pid);
            }
        }
        // A loss is counted once it lasts (crate::LOST_AFTER): say so when it turns, so the pages and Needs you
        // hear of it then, not at the follower's next retry.
        let now = self.shared.o.clock.now();
        for (k, s) in &mut inner.stacks {
            let lost = s.with_state(|st| st.view.link.lost(now));
            if lost != s.lost_seen {
                s.lost_seen = lost;
                changes.push(Changed::stack(k, Parts::CONN));
            }
        }
        // The socket is gone: the daemon exited and cleaned up. What it last showed is dropped with it (its
        // thread stops); the record shows it as stopped.
        let before = inner.stacks.len();
        inner.stacks.retain(|k, _| seen.contains(k));
        list |= inner.stacks.len() != before;
        let mut stopped = BTreeMap::new();
        for p in records {
            let key = StackKey::new(p.dir());
            match inner.stacks.get(&key) {
                Some(s) => changes.push(Changed::stack(&key, s.set_record(&p.last_set))),
                None => {
                    stopped.insert(key, p);
                }
            }
        }
        list |= !inner.refreshed || !same_keys(&stopped, &inner.stopped);
        inner.stopped = stopped;
        inner.refreshed = true;
        drop(inner);
        changes.extend(list.then(Changed::list));
        for c in changes {
            self.changed(c);
        }
    }

    /// Starts following a stack that appeared, streaming what pages already watch.
    pub(super) fn spawn(&self, inner: &Inner, key: &StackKey, socket: &Path) -> Stack {
        let shown: Vec<String> = inner
            .consoles
            .get(key)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        let client = (self.shared.o.dial)(key, socket);
        Stack::start(key.clone(), client, wanted(inner, key), &shown, self.ctx())
    }
}

/// What pages watch of `key` now.
pub(super) fn wanted(inner: &Inner, key: &StackKey) -> Wanted {
    let n = |t: Topic| {
        inner
            .wants
            .get(key)
            .and_then(|m| m.get(&t))
            .copied()
            .unwrap_or(0)
    };
    Wanted {
        stats: !inner.hidden && n(Topic::Stats) > 0,
        // Logs keep coming while hidden: the pages read the hub's ring (Go fetched its window again), so a line said
        // meanwhile must be in it.
        logs: n(Topic::Logs) > 0,
    }
}

fn same_keys(a: &BTreeMap<StackKey, Project>, b: &BTreeMap<StackKey, Project>) -> bool {
    a.len() == b.len() && a.keys().zip(b.keys()).all(|(x, y)| x == y)
}
