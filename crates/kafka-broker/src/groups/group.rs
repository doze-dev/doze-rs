//! The group an actor owns: a classic group, a consumer group or a share group, whichever the id has been used as. A
//! classic or consumer group whose last member goes is free to become the other of the two; one with members moves
//! between them only by the rules in `migrate` (a consumer-protocol member joining a classic group of consumers, the
//! last one leaving). A share group stays one (Kafka converts nothing to or from it): every other kind of request to it
//! is refused, and a share heartbeat to another kind of group is. A streams group likewise stays one; a streams member
//! joining an empty classic group (one that has only committed offsets, say) makes it a streams group, as Kafka does.

use super::consumer::{self, ConsumerGroup, TopicSource};
use super::machine::{Answer, Effect, Event, GroupMachine, IdSource, JoinRequest, Millis};
use super::share::{ShareAnswer, ShareGroup};
use super::streams::{StreamsAnswer, StreamsGroup};
use kafka_meta::{ConsumerLife, ShareLife, StreamsLife};
use kafka_wire::ResponseError as E;

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
    Share(ShareGroup),
    Streams(StreamsGroup),
}

/// What is kept of a group across a restart, by kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kept {
    Classic,
    Consumer(ConsumerLife),
    Share(ShareLife),
    Streams(StreamsLife),
}

impl Kept {
    /// Which kind, for telling a change of kind.
    #[must_use]
    pub(crate) fn tag(&self) -> u8 {
        match self {
            Self::Classic => 0,
            Self::Consumer(_) => 1,
            Self::Share(_) => 2,
            Self::Streams(_) => 3,
        }
    }
}

/// What an event needs written down before the group seats a member: the longest session and the kind of group.
pub(crate) struct Seating {
    pub(crate) session_ms: i32,
    pub(crate) protocol_type: String,
    /// What the group is, or is to be.
    pub(crate) kept: Kept,
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

/// A group of any kind.
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

    /// A share group: new, or as it was when the broker last stopped.
    #[must_use]
    pub(crate) fn share(settings: Settings, kept: Option<&ShareLife>) -> Self {
        let group = kept.map_or_else(ShareGroup::new, |l| {
            ShareGroup::restored(l.epoch, l.assignment_epoch)
        });
        Self {
            kind: Kind::Share(group),
            settings,
        }
    }

    /// A streams group: new, or as it was when the broker last stopped.
    #[must_use]
    pub(crate) fn streams(settings: Settings, kept: Option<&StreamsLife>) -> Self {
        let group = kept.map_or_else(StreamsGroup::new, |l| {
            StreamsGroup::restored(super::streams::stored::kept_of(l))
        });
        Self {
            kind: Kind::Streams(group),
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
            Kind::Share(s) => s.member_count(),
            Kind::Streams(s) => s.member_count(),
        }
    }

    /// The protocol type the members share.
    #[must_use]
    pub(crate) fn protocol_type(&self) -> Option<&str> {
        match &self.kind {
            Kind::Classic(m) => m.protocol_type(),
            Kind::Consumer(_) => Some("consumer"),
            Kind::Share(_) => Some("share"),
            Kind::Streams(_) => Some("streams"),
        }
    }

    /// The epochs a streams group's record is kept up to date with while it has members, so that a broker stopped with
    /// members in it starts the group again at the epoch it had (the epoch never goes back). `None` for the other kinds,
    /// whose record is written as members come and go.
    #[must_use]
    pub(crate) fn kept_epochs(&self) -> Option<(i32, i32)> {
        match &self.kind {
            Kind::Streams(s) => Some(s.epochs()),
            Kind::Classic(_) | Kind::Consumer(_) | Kind::Share(_) => None,
        }
    }

    /// Which kind it is, as [`Kept::tag`] says it.
    #[must_use]
    pub(crate) fn kind_tag(&self) -> u8 {
        match &self.kind {
            Kind::Classic(_) => 0,
            Kind::Consumer(_) => 1,
            Kind::Share(_) => 2,
            Kind::Streams(_) => 3,
        }
    }

    /// What the group keeps across a restart.
    #[must_use]
    pub(crate) fn kept(&self) -> Kept {
        match &self.kind {
            Kind::Classic(_) => Kept::Classic,
            Kind::Consumer(c) => {
                let (epoch, assignment_epoch, assignor) = c.epochs();
                Kept::Consumer(ConsumerLife {
                    epoch,
                    assignment_epoch,
                    assignor: assignor.name().to_owned(),
                })
            }
            Kind::Share(s) => {
                let (epoch, assignment_epoch) = s.epochs();
                Kept::Share(ShareLife {
                    epoch,
                    assignment_epoch,
                })
            }
            Kind::Streams(s) => Kept::Streams(super::streams::stored::life_of(&s.kept(None))),
        }
    }

    fn consumer_kept(&self) -> Kept {
        match self.kept() {
            Kept::Classic | Kept::Share(_) | Kept::Streams(_) => Kept::Consumer(default_life()),
            consumer => consumer,
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
                kept: Kept::Classic,
            }),
            (Event::Join(_, req), Kind::Consumer(c)) => {
                let seats = c.member_count() > 0
                    || GroupMachine::new(0, (self.settings.ids)()).would_seat(req);
                seats.then(|| Seating {
                    session_ms: req.session_ms,
                    protocol_type: req.protocol_type.clone(),
                    kept: self.kept(),
                })
            }
            (Event::ShareHeartbeat(_, hb), Kind::Share(s)) if s.would_seat(hb) => Some(Seating {
                session_ms: i32::try_from(hb.timing.session_ms).unwrap_or(i32::MAX),
                protocol_type: "share".to_owned(),
                kept: self.kept(),
            }),
            (Event::StreamsHeartbeat(_, hb), Kind::Streams(s)) if s.would_seat(hb) => {
                Some(Seating {
                    session_ms: i32::try_from(hb.timing.session_ms).unwrap_or(i32::MAX),
                    protocol_type: "streams".to_owned(),
                    kept: Kept::Streams(super::streams::stored::life_of(
                        &s.kept(hb.topology.as_ref()),
                    )),
                })
            }
            (Event::StreamsHeartbeat(_, hb), Kind::Classic(m))
                if m.member_count() == 0 && hb.epoch == 0 =>
            {
                let new = StreamsGroup::new();
                new.would_seat(hb).then(|| Seating {
                    session_ms: i32::try_from(hb.timing.session_ms).unwrap_or(i32::MAX),
                    protocol_type: "streams".to_owned(),
                    kept: Kept::Streams(super::streams::stored::life_of(
                        &new.kept(hb.topology.as_ref()),
                    )),
                })
            }
            (Event::ConsumerHeartbeat(_, hb), kind)
                if match kind {
                    Kind::Consumer(c) => c.would_seat(hb),
                    Kind::Classic(m) => {
                        (m.member_count() == 0 || m.protocol_type() == Some("consumer"))
                            && consumer::can_join(hb)
                    }
                    Kind::Share(_) | Kind::Streams(_) => false,
                } =>
            {
                Some(Seating {
                    session_ms: i32::try_from(self.settings.consumer.session_ms)
                        .unwrap_or(i32::MAX),
                    protocol_type: "consumer".to_owned(),
                    kept: self.consumer_kept(),
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
        if let Kind::Share(s) = &mut self.kind {
            return step_share(s, event, now, source);
        }
        if let Kind::Streams(s) = &mut self.kind {
            return step_streams(s, event, now, source);
        }
        match event {
            // A client asking for an id before it joins (JoinGroup v4 and later): vetted, and the id issued by the registry.
            Event::Join(token, req) if asks_for_an_id(&req) => {
                vec![Effect::Reply(token, Answer::Join(self.vet(&req)))]
            }
            Event::ConsumerHeartbeat(token, hb) => self.consumer_heartbeat(token, hb, now, source),
            // A streams member joining a classic group nobody is in: it is a streams group now.
            Event::StreamsHeartbeat(token, hb)
                if hb.epoch == 0
                    && matches!(&self.kind, Kind::Classic(m) if m.member_count() == 0) =>
            {
                self.kind = Kind::Streams(StreamsGroup::new());
                self.step(Event::StreamsHeartbeat(token, hb), now, source)
            }
            // A streams heartbeat to any other classic or consumer group.
            Event::StreamsHeartbeat(token, hb) => vec![Effect::Reply(
                token,
                Answer::StreamsHeartbeat(Box::new(StreamsAnswer::refused(
                    E::GroupIdNotFound,
                    format!("Group {} is not a streams group.", hb.group),
                ))),
            )],
            // A share heartbeat to a classic or consumer group, even an empty one (Kafka converts neither).
            Event::ShareHeartbeat(token, hb) => vec![Effect::Reply(
                token,
                Answer::ShareHeartbeat(ShareAnswer::refused(
                    E::GroupIdNotFound,
                    format!("Group {} is not a share group.", hb.group),
                )),
            )],
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
                Kind::Share(_) | Kind::Streams(_) => Vec::new(),
            },
        }
    }
}

mod convert;
mod share_step;
use share_step::step_share;
mod streams_step;
use streams_step::step_streams;
#[cfg(test)]
mod tests;
