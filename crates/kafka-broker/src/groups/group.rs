//! The group an actor owns: a classic group or a consumer group, whichever the id has been used as. A group whose
//! last member goes is free to become the other kind; one with members moves between them only by the rules in
//! `migrate` (a consumer-protocol member joining a classic group of consumers, the last one leaving).

use kafka_wire::ResponseError as E;

use super::consumer::{self, ConsumerGroup, Heartbeat, HeartbeatAnswer, TopicSource};
use super::machine::{
    Answer, Effect, Event, GroupMachine, IdSource, JoinAnswer, JoinRequest, Millis, Token,
};
use kafka_meta::ConsumerLife;

/// How the groups of a broker are made.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settings {
    /// How long the first rebalance of an empty classic group waits for more members.
    pub(crate) initial_delay_ms: u64,
    pub(crate) consumer: consumer::Config,
    /// Makes the source of member ids a new machine needs.
    pub(crate) ids: fn() -> IdSource,
}

enum Kind {
    Classic(GroupMachine),
    Consumer(ConsumerGroup),
}

/// What an event needs written down before the group seats a member: the longest session and the kind of group.
pub(crate) struct Seating {
    pub(crate) session_ms: i32,
    pub(crate) protocol_type: String,
    /// Set if the group is, or is to be, a consumer group of the new protocol.
    pub(crate) consumer: Option<ConsumerLife>,
}

/// A consumer group nobody has been in yet.
fn default_life() -> ConsumerLife {
    ConsumerLife {
        epoch: 1,
        assignment_epoch: 1,
        assignor: "uniform".to_owned(),
    }
}

/// A JoinGroup from a client with no member id, from a version that hands one out first.
pub(crate) fn asks_for_an_id(req: &JoinRequest) -> bool {
    req.id_first && req.member_id.is_none() && req.instance.is_none()
}

/// A group of either kind.
pub(crate) struct Group {
    kind: Kind,
    settings: Settings,
}

impl Group {
    #[must_use]
    pub(crate) fn classic(settings: Settings) -> Self {
        Self {
            kind: Kind::Classic(GroupMachine::new(
                settings.initial_delay_ms,
                (settings.ids)(),
            )),
            settings,
        }
    }

    #[must_use]
    pub(crate) fn consumer(settings: Settings) -> Self {
        Self {
            kind: Kind::Consumer(ConsumerGroup::new(settings.consumer, (settings.ids)())),
            settings,
        }
    }

    /// An empty consumer group as it was when the broker last stopped.
    #[must_use]
    pub(crate) fn restored(
        settings: Settings,
        epoch: i32,
        target_epoch: i32,
        assignor: consumer::assign::Assignor,
    ) -> Self {
        Self {
            kind: Kind::Consumer(ConsumerGroup::restored(
                settings.consumer,
                (settings.ids)(),
                epoch,
                target_epoch,
                assignor,
            )),
            settings,
        }
    }

    #[must_use]
    pub(crate) fn member_count(&self) -> usize {
        match &self.kind {
            Kind::Classic(m) => m.member_count(),
            Kind::Consumer(c) => c.member_count(),
        }
    }

    /// The protocol type the members share.
    #[must_use]
    pub(crate) fn protocol_type(&self) -> Option<&str> {
        match &self.kind {
            Kind::Classic(m) => m.protocol_type(),
            Kind::Consumer(_) => Some("consumer"),
        }
    }

    /// What a consumer group keeps across a restart; `None` for a classic group.
    #[must_use]
    pub(crate) fn consumer_life(&self) -> Option<ConsumerLife> {
        match &self.kind {
            Kind::Classic(_) => None,
            Kind::Consumer(c) => {
                let (epoch, assignment_epoch, assignor) = c.epochs();
                Some(ConsumerLife {
                    epoch,
                    assignment_epoch,
                    assignor: assignor.name().to_owned(),
                })
            }
        }
    }

    /// What must be recorded of the group before this event may seat a member, if it can.
    #[must_use]
    pub(crate) fn seating(&self, event: &Event) -> Option<Seating> {
        match (event, &self.kind) {
            // Asking for an id seats nobody.
            (Event::Join(_, req), _) if asks_for_an_id(req) => None,
            (Event::Join(_, req), Kind::Classic(m)) => m.would_seat(req).then(|| Seating {
                session_ms: req.session_ms,
                protocol_type: req.protocol_type.clone(),
                consumer: None,
            }),
            (Event::Join(_, req), Kind::Consumer(c)) => {
                let seats = c.member_count() > 0
                    || GroupMachine::new(0, (self.settings.ids)()).would_seat(req);
                seats.then(|| Seating {
                    session_ms: req.session_ms,
                    protocol_type: req.protocol_type.clone(),
                    consumer: None,
                })
            }
            (Event::ConsumerHeartbeat(_, hb), kind)
                if match kind {
                    Kind::Consumer(c) => c.would_seat(hb),
                    Kind::Classic(m) => m.member_count() == 0 && consumer::can_join(hb),
                } =>
            {
                Some(Seating {
                    session_ms: i32::try_from(self.settings.consumer.session_ms)
                        .unwrap_or(i32::MAX),
                    protocol_type: "consumer".to_owned(),
                    consumer: Some(self.consumer_life().unwrap_or_else(default_life)),
                })
            }
            _ => None,
        }
    }

    /// Handles one event at time `now`.
    pub(crate) fn step(
        &mut self,
        event: Event,
        now: Millis,
        source: &dyn TopicSource,
    ) -> Vec<Effect> {
        match event {
            // A client asking for an id before it joins (JoinGroup v4 and later): vetted, and the id issued by the registry.
            Event::Join(token, req) if asks_for_an_id(&req) => {
                vec![Effect::Reply(token, Answer::Join(self.vet(&req)))]
            }
            Event::ConsumerHeartbeat(token, hb) => self.consumer_heartbeat(token, hb, now, source),
            // A classic member joining a consumer group nobody is in: the id is the classic protocol's now.
            Event::Join(token, req) if matches!(&self.kind, Kind::Consumer(c) if c.member_count() == 0) =>
            {
                self.kind = Kind::Classic(GroupMachine::new(
                    self.settings.initial_delay_ms,
                    (self.settings.ids)(),
                ));
                self.step(Event::Join(token, req), now, source)
            }
            event => match &mut self.kind {
                Kind::Classic(m) => m.step(event, now),
                Kind::Consumer(c) => match event {
                    // Nobody is in it: a classic request gets what an empty classic group says.
                    Event::Sync(..) | Event::Heartbeat(..) | Event::Leave(..)
                        if c.member_count() == 0 =>
                    {
                        GroupMachine::new(0, (self.settings.ids)()).step(event, now)
                    }
                    event => self.step_consumer(event, now, source),
                },
            },
        }
    }

    /// What a client asking for an id is told by this group: refused if a consumer group would refuse its protocols
    /// whatever id it had, else that it is to come back with an id (the registry makes the answer, with the id).
    fn vet(&self, req: &JoinRequest) -> JoinAnswer {
        let refusal = match &self.kind {
            Kind::Consumer(c) if c.member_count() > 0 => c.protocol_refusal(req),
            _ => None,
        };
        JoinAnswer::refused(refusal.unwrap_or(E::MemberIdRequired), None)
    }

    fn consumer_heartbeat(
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
                let words = if hb.epoch < 0 {
                    format!("Group {} not found.", hb.group)
                } else {
                    format!("Consumer group {} not found.", hb.group)
                };
                return refuse(words);
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
    fn step_consumer(
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

#[cfg(test)]
mod tests;
