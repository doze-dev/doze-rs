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
                    epoch: if m.left { -2 } else { m.epoch },
                    subscribed: m.subscribed.clone(),
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
