//! What a consumer group does with each kind of event.

use std::collections::BTreeSet;

use kafka_wire::ResponseError as E;

use super::{ConsumerGroup, TopicSource, Topics};
use crate::groups::machine::{Answer, CommitRequest, Description, Effect, Event, Millis, State};

impl ConsumerGroup {
    /// Handles one event at time `now`.
    pub(crate) fn step(
        &mut self,
        event: Event,
        now: Millis,
        source: &dyn TopicSource,
    ) -> Vec<Effect> {
        let mut out = Vec::new();
        match event {
            Event::ConsumerHeartbeat(token, hb) => {
                self.heartbeat(token, &hb, &source.topics(), now, &mut out);
            }
            Event::Timer(key) => self.timer(&key, now, &mut out),
            Event::TopicsChanged => self.topics_changed(&source.topics(), now, &mut out),
            Event::Describe(token) => out.push(Effect::Reply(
                token,
                Answer::Describe(self.description(&source.topics(), now)),
            )),
            Event::Delete(token) => {
                let refused = (!self.members.is_empty()).then_some(E::NonEmptyGroup);
                out.push(Effect::Reply(token, Answer::Delete(refused)));
            }
            Event::Commit(token, req) => out.push(Effect::Reply(
                token,
                Answer::Commit(self.commit_fence(&req)),
            )),
            // A classic member's requests: see classic.rs.
            Event::Join(token, req) => {
                self.classic_join(token, &req, &source.topics(), now, &mut out)
            }
            Event::Sync(token, req) => {
                self.classic_sync(token, &req, &source.topics(), now, &mut out)
            }
            Event::Heartbeat(token, req) => {
                self.classic_heartbeat(token, &req, &source.topics(), now, &mut out)
            }
            Event::Leave(token, targets) => self.classic_leave(token, &targets, &mut out),
        }
        out
    }

    /// Kafka's rules for a commit to a consumer group: a consumer-protocol member names its epoch (OffsetCommit v9
    /// and later, the first to carry it as such); with nobody in the group only a commit that names no member may go
    /// ahead; otherwise the member must be there, at its own epoch.
    fn commit_fence(&self, req: &CommitRequest) -> Option<E> {
        if req.transactional && req.names_nobody() {
            return None;
        }
        let member = req.member_id.as_ref().and_then(|id| self.members.get(id));
        if req.version < 9 && member.is_some_and(|m| m.classic.is_none()) {
            return Some(E::UnsupportedVersion);
        }
        // Nobody in the group and no generation named: an admin tool, or a consumer that does not use the group (Kafka
        // accepts it whoever it says it is).
        if req.generation < 0 && self.members.is_empty() {
            return None;
        }
        let Some(member) = member else {
            return Some(E::UnknownMemberId);
        };
        if req.generation == member.epoch {
            return None;
        }
        // A classic member's epoch is its generation; a transaction's offsets are refused as a wrong generation by either.
        Some(if member.classic.is_some() || req.transactional {
            E::IllegalGeneration
        } else {
            E::StaleMemberEpoch
        })
    }

    /// The group as the classic APIs and ListGroups see it: not described by the classic APIs, but listed.
    #[cfg_attr(not(feature = "inspect"), allow(unused_variables))]
    fn description(&self, topics: &Topics, now: Millis) -> Description {
        // The names members subscribe to, and the topics that exist which a member's pattern matches.
        let mut subscribed: BTreeSet<String> = self
            .members
            .values()
            .flat_map(|m| m.visible_names().cloned())
            .collect();
        if self.members.values().any(|m| m.pattern.is_some()) {
            subscribed.extend(
                topics
                    .all()
                    .filter(|t| self.members.values().any(|m| m.pattern_takes(&t.name)))
                    .map(|t| t.name.clone()),
            );
        }
        Description {
            state: if self.members.is_empty() {
                State::Empty
            } else {
                State::Stable
            },
            protocol_type: Some("consumer".to_owned()),
            protocol: None,
            members: Vec::new(),
            subscribed: Some(subscribed),
            consumer: Some(self.view()),
            #[cfg(feature = "inspect")]
            inspect: self.inspect(now),
        }
    }
}
