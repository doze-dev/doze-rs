//! How a consumer group is shown.

use super::{ConsumerGroup, ConsumerView, MemberView};

impl ConsumerGroup {
    /// `Empty`, `Assigning` (the members are behind the group's epoch: the targets are yet to be computed),
    /// `Reconciling` (some member is not yet where its target is) or `Stable`.
    fn state_name(&self) -> &'static str {
        if self.members.is_empty() {
            "Empty"
        } else if self.target_epoch < self.epoch {
            "Assigning"
        } else if self.members.values().any(|m| {
            // A static member that has gone away to restart is not at the group's epoch.
            m.left
                || m.epoch != self.target_epoch
                || !m.revoking.is_empty()
                || m.assigned != m.target
        }) {
            "Reconciling"
        } else {
            "Stable"
        }
    }

    #[must_use]
    pub(crate) fn view(&self) -> ConsumerView {
        ConsumerView {
            state: self.state_name(),
            epoch: self.epoch,
            assignment_epoch: self.target_epoch,
            assignor: self.assignor.name(),
            members: self
                .members
                .values()
                .map(|m| MemberView {
                    id: m.id.clone(),
                    instance: m.instance.clone(),
                    rack: m.rack.clone(),
                    client_id: m.client_id.clone(),
                    client_host: m.client_host.clone(),
                    epoch: if m.left { -2 } else { m.epoch },
                    // Only the names its own principal may DESCRIBE (SEC-232).
                    subscribed: m.visible_names().cloned().collect(),
                    regex: m
                        .pattern
                        .as_ref()
                        .map_or_else(String::new, |p| p.text().to_owned()),
                    // It holds what it has been told to give up until it says it has.
                    assigned: m.assigned.union(&m.revoking).copied().collect(),
                    target: m.target.clone(),
                    classic: m.classic.is_some(),
                })
                .collect(),
        }
    }
}

#[cfg(feature = "inspect")]
impl ConsumerGroup {
    /// What the console is told of the group besides what ConsumerGroupDescribe says.
    pub(crate) fn inspect(&self, now: crate::groups::machine::Millis) -> crate::groups::Inspect {
        use crate::groups::inspect::MemberInspect;
        crate::groups::Inspect {
            generation: self.epoch,
            leader: None,
            rebalance: None,
            members: self
                .members
                .values()
                .map(|m| MemberInspect {
                    id: m.id.clone(),
                    session_ms: m
                        .classic
                        .as_ref()
                        .map_or(self.cfg.session_ms, |c| c.session_ms),
                    rebalance_ms: u64::try_from(m.rebalance_ms).ok().filter(|ms| *ms > 0),
                    heard_ago_ms: now.0.saturating_sub(m.heard.0),
                    rejoined: true,
                })
                .collect(),
        }
    }
}
