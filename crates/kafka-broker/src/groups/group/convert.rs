//! A group changing kind: a classic group of consumers becomes a consumer group when a member of the new protocol joins
//! it, and is classic again when the last of those is gone. A request that is refused changes nothing.

use kafka_wire::ResponseError as E;
use kafka_wire::ids::MemberId;

use super::super::consumer::{self, ConsumerGroup, Heartbeat, HeartbeatAnswer, TopicSource};
use super::super::machine::{
    Answer, Effect, Event, GroupMachine, JoinAnswer, JoinRequest, Millis, Token,
};
use super::{Group, Kind};

impl Group {
    /// What a client asking for an id is told by this group: refused if a consumer group would refuse its protocols
    /// whatever id it had, else that it is to come back with an id (the registry makes the answer, with the id).
    pub(super) fn vet(&self, req: &JoinRequest) -> JoinAnswer {
        let refusal = match &self.kind {
            Kind::Consumer(c) if c.member_count() > 0 => c.protocol_refusal(req),
            _ => None,
        };
        JoinAnswer::refused(refusal.unwrap_or(E::MemberIdRequired), None)
    }

    pub(super) fn consumer_heartbeat(
        &mut self,
        token: Token,
        hb: Heartbeat,
        now: Millis,
        source: &dyn TopicSource,
    ) -> Vec<Effect> {
        let refuse = |words: String| {
            vec![Effect::Reply(
                token,
                Answer::ConsumerHeartbeat(HeartbeatAnswer::refused(E::GroupIdNotFound, words)),
            )]
        };
        let mut out = Vec::new();
        if let Kind::Classic(m) = &mut self.kind {
            // To the classic coordinator a consumer group that is not there is not there.
            if hb.epoch != 0 {
                return refuse(format!("Group {} is not a consumer group.", hb.group));
            }
            // A request that is refused changes nothing: the group keeps its kind and its members.
            if let Some(refused) = consumer::join_refusal(&hb) {
                return vec![Effect::Reply(token, Answer::ConsumerHeartbeat(refused))];
            }
            if let Some(instance) = &hb.instance
                && m.member_count() > 0
                && let Some(holder) = m.holder_of(instance)
            {
                let words = format!(
                    "Static member {} with instance id {instance} cannot join the group because the instance id is owned by {holder} member.",
                    hb.member_id.as_ref().map_or("", MemberId::as_str)
                );
                let refused = HeartbeatAnswer::refused(E::UnreleasedInstanceId, words);
                return vec![Effect::Reply(token, Answer::ConsumerHeartbeat(refused))];
            }
            if m.member_count() > consumer::MOST_MEMBERS {
                return refuse(format!(
                    "Cannot upgrade classic group {} to consumer group because the group size exceeds the \
                     consumer group maximum size.",
                    hb.group
                ));
            }
            if m.member_count() == 0 {
                self.kind = Kind::Consumer(ConsumerGroup::new(
                    self.settings.consumer,
                    (self.settings.ids)(),
                ));
            } else {
                // A group of consumers with members becomes a consumer group, its members staying as members of it. A
                // group of another protocol type (Connect), or one whose subscriptions cannot be read, is not upgraded.
                let Some(handover) = m.hand_over(&mut out) else {
                    return refuse(format!(
                        "Cannot upgrade classic group {} to consumer group because the group does not use the \
                         consumer embedded protocol.",
                        hb.group
                    ));
                };
                let adopted = ConsumerGroup::adopt(
                    self.settings.consumer,
                    (self.settings.ids)(),
                    handover,
                    &source.topics(),
                    now,
                    &mut out,
                );
                self.kind = Kind::Consumer(adopted);
            }
        }
        out.extend(self.step_consumer(Event::ConsumerHeartbeat(token, hb), now, source));
        out
    }

    /// A consumer group's step, and the change to a classic group if its last consumer-protocol member has gone.
    pub(super) fn step_consumer(
        &mut self,
        event: Event,
        now: Millis,
        source: &dyn TopicSource,
    ) -> Vec<Effect> {
        let Kind::Consumer(c) = &mut self.kind else {
            return Vec::new();
        };
        let mut out = c.step(event, now, source);
        if c.take_downgrade()
            && let Some(handover) = c.hand_back(&source.topics(), &mut out)
        {
            let initial = self.settings.initial_delay_ms;
            let classic = GroupMachine::from_handover(
                initial,
                (self.settings.ids)(),
                handover,
                now,
                &mut out,
            );
            self.kind = Kind::Classic(classic);
        }
        out
    }
}
