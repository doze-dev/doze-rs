//! What this app is doing to a stack, as the hub tells the pages: while an action of the app is in flight (up, down,
//! restart, a wake that starts the stack), the daemon starting or exiting under it is the action's own doing, so its
//! connection dropping is not a daemon that stopped answering ([`crate::StackView::daemon_lost`]).

use super::Hub;
use crate::changed::{Changed, Parts};
use crate::stack::lock;
use doze_projects::StackKey;

impl Hub {
    /// Says an action of this app on the stack `key` began (`on`) or ended. When it ends, a loss is counted from
    /// then ([`crate::Link::settle`]), and a stack whose connection is not live is followed afresh: the daemon the
    /// action started is dialled now, at Connecting, not at the follower's next retry after the last one's Down.
    pub fn set_acting(&self, key: &StackKey, on: bool) {
        let mut inner = lock(&self.shared.inner);
        let changed = if on {
            inner.acting.insert(key.clone())
        } else {
            inner.acting.remove(key)
        };
        if !on && let Some(s) = inner.stacks.get(key) {
            let now = self.shared.o.clock.now();
            let live = s.with_state(|st| {
                st.view.link.settle(now);
                st.view.link.is_live()
            });
            if !live {
                let pid = s.pid;
                let mut fresh = self.spawn(&inner, key, &key.socket());
                fresh.pid = pid;
                inner.stacks.insert(key.clone(), fresh);
            }
        }
        drop(inner);
        if changed {
            self.changed(Changed::stack(key, Parts::CONN));
        }
    }

    /// Whether an action of this app is in flight on the stack `key`.
    pub fn acting(&self, key: &StackKey) -> bool {
        lock(&self.shared.inner).acting.contains(key)
    }
}
