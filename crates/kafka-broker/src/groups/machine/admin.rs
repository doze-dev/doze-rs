//! What DescribeGroups, ListGroups and DeleteGroups ask of a group.

use std::collections::BTreeSet;

use bytes::Bytes;
use kafka_wire::ResponseError;

use super::{Answer, Description, Effect, GroupMachine, MemberDescription, State, Token};
use crate::groups::subscription;

impl GroupMachine {
    pub(super) fn describe(&self, token: Token, out: &mut Vec<Effect>) {
        out.push(Effect::Reply(token, Answer::Describe(self.description())));
    }

    /// A group with members cannot be deleted; an empty one can, and the driver then lets it go.
    pub(super) fn delete(&self, token: Token, out: &mut Vec<Effect>) {
        let refused = (!self.members.is_empty()).then_some(ResponseError::NonEmptyGroup);
        out.push(Effect::Reply(token, Answer::Delete(refused)));
    }

    fn description(&self) -> Description {
        let chosen = self.protocol.as_deref();
        let members: Vec<MemberDescription> = self
            .members
            .values()
            .map(|m| MemberDescription {
                id: m.id.clone(),
                instance: m.instance.clone(),
                client_id: m.client_id.clone(),
                metadata: m
                    .protocols
                    .iter()
                    .find(|p| Some(p.name.as_str()) == chosen)
                    .map(|p| p.metadata.clone())
                    .unwrap_or_default(),
                assignment: m.assignment.clone(),
            })
            .collect();
        Description {
            state: self.state,
            protocol_type: self.protocol_type.clone(),
            protocol: self.protocol.clone(),
            subscribed: self.subscribed(&members),
            consumer: None,
            members,
        }
    }

    /// The topics the members subscribe to, if they are a consumer group and every member's can be read.
    fn subscribed(&self, members: &[MemberDescription]) -> Option<BTreeSet<String>> {
        if self.state == State::Empty {
            return Some(BTreeSet::new());
        }
        if self.protocol_type.as_deref() != Some("consumer") {
            return None;
        }
        let mut all = BTreeSet::new();
        for m in members {
            all.extend(subscription::topics(&Bytes::clone(&m.metadata))?);
        }
        Some(all)
    }
}
