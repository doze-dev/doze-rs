//! Who may heartbeat into a share group (Kafka's size and epoch checks), and the member as its heartbeat describes it.

use std::collections::BTreeSet;

use kafka_wire::ResponseError as E;

use super::{Member, ShareAnswer, ShareGroup, ShareHeartbeat, not_a_member};
use crate::groups::consumer::assign::Partitions;
use crate::groups::machine::Millis;

impl ShareGroup {
    /// Kafka's checks of a heartbeat before it changes anything: the group's size, and a known member's epoch.
    pub(super) fn refusal(&self, hb: &ShareHeartbeat) -> Option<ShareAnswer> {
        let id = &hb.member_id;
        let Some(m) = self.members.get(id) else {
            if self.members.len() >= hb.timing.max_size {
                return Some(ShareAnswer::refused(
                    E::GroupMaxSizeReached,
                    format!(
                        "The share group has reached its maximum capacity of {} members.",
                        hb.timing.max_size
                    ),
                ));
            }
            return (hb.epoch != 0).then(|| {
                ShareAnswer::refused(E::UnknownMemberId, not_a_member(id.as_str(), &hb.group))
            });
        };
        epoch_refusal(hb.epoch, m)
    }

    /// The member as the heartbeat describes it, seated if it is new. `true`: its subscription changed.
    pub(super) fn seat(&mut self, hb: &ShareHeartbeat, now: Millis) -> bool {
        let id = &hb.member_id;
        let m = self.members.entry(id.clone()).or_insert_with(|| Member {
            id: id.clone(),
            epoch: 0,
            prev_epoch: -1,
            rack: None,
            client_id: String::new(),
            client_host: String::new(),
            subscribed: BTreeSet::new(),
            assigned: Partitions::new(),
            session_gen: 0,
            session_ms: hb.timing.session_ms,
            heard: now,
        });
        if let Some(rack) = &hb.rack {
            m.rack = Some(rack.clone());
        }
        m.client_id.clone_from(&hb.client_id);
        m.client_host.clone_from(&hb.client_host);
        m.session_ms = hb.timing.session_ms;
        let Some(names) = &hb.subscribed else {
            return false;
        };
        let names: BTreeSet<String> = names.iter().cloned().collect();
        if m.subscribed == names {
            return false;
        }
        m.subscribed = names;
        true
    }
}

/// Kafka's epoch check of a known member: 0 rejoins, the epoch it is at or the one before (an answer lost) go on.
fn epoch_refusal(sent: i32, m: &Member) -> Option<ShareAnswer> {
    if sent == 0 || sent == m.epoch || sent == m.prev_epoch {
        return None;
    }
    let words = if sent > m.epoch {
        format!(
            "The share group member has a greater member epoch ({sent}) than the one known by the group coordinator \
             ({}). The member must abandon all its partitions and rejoin.",
            m.epoch
        )
    } else {
        format!(
            "The share group member has a smaller member epoch ({sent}) than the one known by the group coordinator \
             ({}), and it does not match the previous member epoch ({}). The member must abandon all its partitions \
             and rejoin.",
            m.epoch, m.prev_epoch
        )
    };
    Some(ShareAnswer::refused(E::FencedMemberEpoch, words))
}
