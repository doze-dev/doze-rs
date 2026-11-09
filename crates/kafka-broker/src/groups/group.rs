//! The group an actor owns: a classic group or a consumer group, whichever the id has been used as. A group whose
//! last member goes is free to become the other kind; one with members moves between them only by the rules in
//! `migrate` (a consumer-protocol member joining a classic group of consumers, the last one leaving).

use kafka_wire::ResponseError as E;

use super::consumer::{self, ConsumerGroup, Heartbeat, HeartbeatAnswer, TopicSource};
use super::machine::{Answer, Effect, Event, GroupMachine, IdSource, Millis, Token};

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
    #[expect(dead_code, reason = "slice 5f restores groups at start")]
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

    /// What must be recorded of the group before this event may seat a member, if it can.
    #[must_use]
    pub(crate) fn seating(&self, event: &Event) -> Option<Seating> {
        match (event, &self.kind) {
            (Event::Join(_, req), Kind::Classic(m)) => m.would_seat(req).then(|| Seating {
                session_ms: req.session_ms,
                protocol_type: req.protocol_type.clone(),
            }),
            (Event::Join(..), Kind::Consumer(_)) => None,
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
                    event => c.step(event, now, source),
                },
            },
        }
    }

    fn consumer_heartbeat(
        &mut self,
        token: Token,
        hb: Heartbeat,
        now: Millis,
        source: &dyn TopicSource,
    ) -> Vec<Effect> {
        let not_found = |hb: &Heartbeat| {
            let words = if hb.epoch < 0 {
                format!("Group {} not found.", hb.group)
            } else {
                format!("Consumer group {} not found.", hb.group)
            };
            vec![Effect::Reply(
                token,
                Answer::ConsumerHeartbeat(HeartbeatAnswer::refused(E::GroupIdNotFound, words)),
            )]
        };
        match &self.kind {
            Kind::Consumer(_) => {}
            // To the classic coordinator a consumer group that is not there is not there.
            Kind::Classic(_) if hb.epoch != 0 => return not_found(&hb),
            Kind::Classic(m) if m.member_count() == 0 => {
                self.kind = Kind::Consumer(ConsumerGroup::new(
                    self.settings.consumer,
                    (self.settings.ids)(),
                ));
            }
            Kind::Classic(m) if m.protocol_type() != Some("consumer") => {
                let words = format!(
                    "Cannot upgrade classic group {} to consumer group because the group does not use the consumer \
                     embedded protocol.",
                    hb.group
                );
                return vec![Effect::Reply(
                    token,
                    Answer::ConsumerHeartbeat(HeartbeatAnswer::refused(E::GroupIdNotFound, words)),
                )];
            }
            Kind::Classic(_) => {
                let words = "the group id is in use by a classic group";
                return vec![Effect::Reply(
                    token,
                    Answer::ConsumerHeartbeat(HeartbeatAnswer::refused(E::GroupIdNotFound, words)),
                )];
            }
        }
        match &mut self.kind {
            Kind::Consumer(c) => c.step(Event::ConsumerHeartbeat(token, hb), now, source),
            Kind::Classic(_) => Vec::new(),
        }
    }
}
