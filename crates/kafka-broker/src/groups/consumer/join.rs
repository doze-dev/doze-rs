//! Joining: what a request can get wrong whatever the group, and seating a member that joins.

use kafka_wire::ResponseError as E;
use kafka_wire::ids::MemberId;

use super::assign::Assignor;
use super::member::Member;
use super::pattern::Pattern;
use super::{ConsumerGroup, Heartbeat, HeartbeatAnswer};
use crate::groups::machine::Effect;

/// The most members a consumer group takes (Kafka's `group.consumer.max.size`).
pub(super) const MOST_MEMBERS: usize = 1000;

/// What a request asks for that can be wrong whatever the group.
pub(super) struct Parsed {
    pub(super) assignor: Option<Assignor>,
    pub(super) pattern: Option<Pattern>,
}

/// The assignor and the pattern a request names, or the words they are refused in.
pub(super) fn parse(req: &Heartbeat) -> Result<Parsed, HeartbeatAnswer> {
    let assignor = match req.assignor.as_deref() {
        None | Some("") => None,
        Some(name) => match Assignor::parse(name) {
            Some(a) => Some(a),
            None => {
                return Err(HeartbeatAnswer::refused(
                    E::UnsupportedAssignor,
                    format!(
                        "ServerAssignor {name} is not supported. Supported assignors: uniform, range."
                    ),
                ));
            }
        },
    };
    let pattern = match req.regex.as_deref() {
        None | Some("") => None,
        Some(t) => Some(
            Pattern::compile(t)
                .map_err(|why| HeartbeatAnswer::refused(E::InvalidRegularExpression, why))?,
        ),
    };
    Ok(Parsed { assignor, pattern })
}

/// Whether a join can seat a member whatever the group it comes to: it names an id (or is version 0, which is given
/// one) and asks for nothing that is refused. Whether a group with members will seat it is `ConsumerGroup::would_seat`.
pub(crate) fn can_join(req: &Heartbeat) -> bool {
    req.epoch == 0 && join_refusal(req).is_none()
}

/// Why a join is refused whatever the group it comes to, in Kafka's words. Checked before a group changes kind for
/// it: a request that fails must not disturb a group.
pub(crate) fn join_refusal(req: &Heartbeat) -> Option<HeartbeatAnswer> {
    if req.member_id.is_none() && req.version != 0 {
        return Some(HeartbeatAnswer::refused(
            E::InvalidRequest,
            "a member id is required",
        ));
    }
    parse(req).err()
}

impl ConsumerGroup {
    /// Whether this join will seat a member: it can, and its static instance is not held by a live member.
    pub(crate) fn would_seat(&self, req: &Heartbeat) -> bool {
        can_join(req)
            && req.instance.as_ref().is_none_or(|i| {
                self.members.values().all(|o| {
                    o.instance.as_ref() != Some(i)
                        || o.left
                        || Some(&o.id) == req.member_id.as_ref()
                })
            })
    }

    /// Seats a member that joins (epoch 0): a new one, one that is back, or a static instance's restarted self, which
    /// takes the place of the member it left behind, partitions and all.
    pub(super) fn seat(
        &mut self,
        id: &MemberId,
        req: &Heartbeat,
        changed: &mut bool,
        out: &mut Vec<Effect>,
    ) -> Result<(), HeartbeatAnswer> {
        if self.members.len() >= MOST_MEMBERS && !self.members.contains_key(id) {
            let words = format!(
                "The consumer group has reached its maximum size of {MOST_MEMBERS} members."
            );
            return Err(HeartbeatAnswer::refused(E::GroupMaxSizeReached, words));
        }
        let mut member = self.members.remove(id);
        if let Some(instance) = &req.instance
            && let Some(other_id) = self
                .members
                .values()
                .find(|o| o.instance.as_ref() == Some(instance))
                .map(|o| o.id.clone())
        {
            let held_by_live_member = self.members.get(&other_id).is_some_and(|o| !o.left);
            if held_by_live_member {
                if let Some(m) = member {
                    self.members.insert(id.clone(), m);
                }
                return Err(HeartbeatAnswer::refused(
                    E::UnreleasedInstanceId,
                    format!(
                        "Static member {id} with instance id {instance} cannot join the group because the instance \
                         id is owned by {other_id} member."
                    ),
                ));
            }
            if let Some(old) = self.members.remove(&other_id) {
                Self::disarm(&other_id, out);
                let mut taken = Member::new(id.clone());
                taken.target = old.target;
                taken.assigned = old.assigned;
                taken.revoking = old.revoking;
                taken.epoch = old.epoch;
                taken.prev_epoch = old.prev_epoch;
                taken.subscribed = old.subscribed;
                taken.pattern = old.pattern;
                taken.assignor = old.assignor;
                member = Some(taken);
            }
        }
        let mut member = match member {
            None => {
                *changed = true;
                Member::new(id.clone())
            }
            Some(mut known) => {
                // A known member joining at epoch 0 has dropped everything it held.
                if known.sent.is_some() && !known.left {
                    known.assigned.clear();
                    known.revoking.clear();
                    known.revoke_gen = 0;
                    *changed = true;
                    out.push(Effect::Disarm(crate::groups::machine::TimerKind::Revoke(
                        id.clone(),
                    )));
                }
                known
            }
        };
        member.left = false;
        member.sent = None;
        member.rebalance_ms = req.rebalance_ms;
        self.members.insert(id.clone(), member);
        Ok(())
    }
}
