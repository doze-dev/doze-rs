//! A share group as ShareGroupDescribe and the console see it.

use super::{ShareGroup, ShareMemberView, ShareView};
use crate::groups::consumer::Topics;
#[cfg(feature = "inspect")]
use crate::groups::machine::Millis;

impl ShareGroup {
    #[must_use]
    pub(crate) fn view(&self, topics: &Topics) -> ShareView {
        ShareView {
            state: if self.members.is_empty() {
                "Empty"
            } else {
                "Stable"
            },
            epoch: self.epoch,
            assignment_epoch: self.assignment_epoch,
            members: self
                .members
                .values()
                .map(|m| ShareMemberView {
                    id: m.id.clone(),
                    rack: m.rack.clone(),
                    epoch: m.epoch,
                    client_id: m.client_id.clone(),
                    client_host: m.client_host.clone(),
                    subscribed: m.subscribed.iter().cloned().collect(),
                    // A topic deleted since is not shown.
                    assigned: m
                        .assigned
                        .iter()
                        .filter(|p| topics.name_of(&p.0).is_some())
                        .copied()
                        .collect(),
                })
                .collect(),
        }
    }

    /// What the console is told of the group besides what ShareGroupDescribe says.
    #[cfg(feature = "inspect")]
    pub(crate) fn inspect(&self, now: Millis) -> crate::groups::Inspect {
        crate::groups::Inspect {
            generation: self.epoch,
            leader: None,
            rebalance: None,
            members: self
                .members
                .values()
                .map(|m| crate::groups::MemberInspect {
                    id: m.id.clone(),
                    session_ms: m.session_ms,
                    rebalance_ms: None,
                    heard_ago_ms: now.0.saturating_sub(m.heard.0),
                    rejoined: true,
                })
                .collect(),
        }
    }
}
