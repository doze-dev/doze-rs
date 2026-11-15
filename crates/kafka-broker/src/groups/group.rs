//! The group an actor owns: a classic group, a consumer group or a share group, whichever the id has been used as. A
//! classic or consumer group whose last member goes is free to become the other of the two; one with members moves
//! between them only by the rules in `migrate` (a consumer-protocol member joining a classic group of consumers, the
//! last one leaving). A share group stays one (Kafka converts nothing to or from it): every other kind of request to it
//! is refused, and a share heartbeat to another kind of group is.

use super::consumer::{self, ConsumerGroup, TopicSource};
use super::machine::{Answer, Effect, Event, GroupMachine, IdSource, JoinRequest, Millis};
use super::share::{ShareAnswer, ShareGroup};
use kafka_meta::{ConsumerLife, ShareLife};
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
}

/// What is kept of a group across a restart, by kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kept {
    Classic,
    Consumer(ConsumerLife),
    Share(ShareLife),
}

impl Kept {
    /// Which kind, for telling a change of kind.
    #[must_use]
    pub(crate) fn tag(&self) -> u8 {
        match self {
            Self::Classic => 0,
            Self::Consumer(_) => 1,
            Self::Share(_) => 2,
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
        }
    }

    /// The protocol type the members share.
    #[must_use]
    pub(crate) fn protocol_type(&self) -> Option<&str> {
        match &self.kind {
            Kind::Classic(m) => m.protocol_type(),
            Kind::Consumer(_) => Some("consumer"),
            Kind::Share(_) => Some("share"),
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
        }
    }

    fn consumer_kept(&self) -> Kept {
        match self.kept() {
            Kept::Classic | Kept::Share(_) => Kept::Consumer(default_life()),
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
            (Event::ConsumerHeartbeat(_, hb), kind)
                if match kind {
                    Kind::Consumer(c) => c.would_seat(hb),
                    Kind::Classic(m) => {
                        (m.member_count() == 0 || m.protocol_type() == Some("consumer"))
                            && consumer::can_join(hb)
                    }
                    Kind::Share(_) => false,
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
        match event {
            // A client asking for an id before it joins (JoinGroup v4 and later): vetted, and the id issued by the registry.
            Event::Join(token, req) if asks_for_an_id(&req) => {
                vec![Effect::Reply(token, Answer::Join(self.vet(&req)))]
            }
            Event::ConsumerHeartbeat(token, hb) => self.consumer_heartbeat(token, hb, now, source),
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
                Kind::Share(_) => Vec::new(),
            },
        }
    }
}

/// A share group's step: its own heartbeat, and Kafka's refusals of every other kind's requests
/// (`GroupMetadataManager`'s `getOrMaybeCreate*Group`: "Group %s is not a ... group.").
fn step_share(s: &mut ShareGroup, event: Event, now: Millis, source: &dyn TopicSource) -> Vec<Effect> {
    use super::machine::{Description, Forced, JoinAnswer, LeaveResult, State, SyncAnswer};
    let mut out = Vec::new();
    match event {
        Event::ShareHeartbeat(token, hb) => s.heartbeat(token, &hb, &source.topics(), now, &mut out),
        Event::Timer(key) => s.timer(&key, now, &mut out),
        Event::TopicsChanged => s.topics_changed(),
        Event::Describe(token) => {
            let view = s.view(&source.topics());
            out.push(Effect::Reply(
                token,
                Answer::Describe(Description {
                    state: if view.members.is_empty() {
                        State::Empty
                    } else {
                        State::Stable
                    },
                    protocol_type: Some("share".to_owned()),
                    protocol: None,
                    members: Vec::new(),
                    subscribed: None,
                    consumer: None,
                    share: Some(view),
                    #[cfg(feature = "inspect")]
                    inspect: Box::new(s.inspect(now)),
                }),
            ));
        }
        Event::Delete(token) => {
            let refused = (s.member_count() > 0).then_some(E::NonEmptyGroup);
            out.push(Effect::Reply(token, Answer::Delete(refused)));
        }
        // A classic JoinGroup on a share group is told its protocol is not the group's.
        Event::Join(token, _) => out.push(Effect::Reply(
            token,
            Answer::Join(JoinAnswer::refused(E::InconsistentGroupProtocol, None)),
        )),
        Event::Sync(token, _) => out.push(Effect::Reply(
            token,
            Answer::Sync(SyncAnswer::refused(E::GroupIdNotFound)),
        )),
        Event::Heartbeat(token, _) => {
            out.push(Effect::Reply(token, Answer::Heartbeat(Some(E::GroupIdNotFound))));
        }
        Event::Leave(token, targets) => out.push(Effect::Reply(
            token,
            Answer::Leave(
                targets
                    .into_iter()
                    .map(|t| LeaveResult {
                        member_id: t.member_id,
                        instance: t.instance,
                        error: Some(E::GroupIdNotFound),
                    })
                    .collect(),
            ),
        )),
        // A share group has no committed offsets: "Group %s is not a consumer group."
        Event::Commit(token, _) => out.push(Effect::Reply(token, Answer::Commit(Some(E::GroupIdNotFound)))),
        Event::ConsumerHeartbeat(token, hb) => out.push(Effect::Reply(
            token,
            Answer::ConsumerHeartbeat(consumer::HeartbeatAnswer::refused(
                E::GroupIdNotFound,
                format!("Group {} is not a consumer group.", hb.group),
            )),
        )),
        Event::Force(token, force) => {
            let done = match force {
                super::machine::Force::Expire(id) => {
                    if s.force_expire(&id, now, &mut out) {
                        Forced::Done
                    } else {
                        Forced::NoSuchMember
                    }
                }
                super::machine::Force::Rebalance => {
                    if s.force_bump(now, &mut out) {
                        Forced::Done
                    } else {
                        Forced::Empty
                    }
                }
            };
            out.push(Effect::Reply(token, Answer::Forced(done)));
        }
    }
    out
}

mod convert;
#[cfg(test)]
mod tests;
