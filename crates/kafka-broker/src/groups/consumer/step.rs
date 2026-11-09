//! What a consumer group does with each kind of event.

use std::collections::BTreeSet;

use kafka_wire::ResponseError as E;

use super::{ConsumerGroup, TopicSource};
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
            Event::TopicsChanged => self.topics_changed(&source.topics(), now),
            Event::Describe(token) => {
                out.push(Effect::Reply(token, Answer::Describe(self.description())))
            }
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
            Event::Heartbeat(token, req) => self.classic_heartbeat(token, &req, now, &mut out),
            Event::Leave(token, targets) => self.classic_leave(token, &targets, &mut out),
        }
        out
    }

    /// Kafka's rules for a commit to a consumer group: a consumer-protocol member names its epoch (OffsetCommit v9
    /// and later, the first to carry it as such); with nobody in the group only a commit that names no member may go
    /// ahead; otherwise the member must be there, at its own epoch.
    fn commit_fence(&self, req: &CommitRequest) -> Option<E> {
        let member = req.member_id.as_ref().and_then(|id| self.members.get(id));
        if req.version < 9 && member.is_some_and(|m| m.classic.is_none()) {
            return Some(E::UnsupportedVersion);
        }
        if self.members.is_empty() {
            return (req.generation >= 0 || req.member_id.is_some()).then_some(E::UnknownMemberId);
        }
        let Some(member) = member else {
            return Some(E::UnknownMemberId);
        };
        if req.generation == member.epoch {
            return None;
        }
        // A classic member's epoch is its generation.
        Some(if member.classic.is_some() {
            E::IllegalGeneration
        } else {
            E::StaleMemberEpoch
        })
    }

    /// The group as the classic APIs and ListGroups see it: not described by the classic APIs, but listed.
    fn description(&self) -> Description {
        let subscribed: BTreeSet<String> = self
            .members
            .values()
            .flat_map(|m| m.subscribed.iter().cloned())
            .collect();
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
        }
    }
}
