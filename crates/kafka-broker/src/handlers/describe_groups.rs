//! DescribeGroups.

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::StrBytes;
use kafka_wire::ids::GroupId;
use kafka_wire::messages::describe_groups_response::{DescribedGroup, DescribedGroupMember};
use kafka_wire::messages::{DescribeGroupsRequest, DescribeGroupsResponse};

use crate::groups::{Description, State, kafka_order};
use crate::{Broker, Caller, Outcome, blocking};

impl<F: Fs> Broker<F> {
    /// Describes each group named, in the order Kafka answers. A group the broker has never heard of is Dead, and one known
    /// only by its committed offsets is Empty. Up to v5 Dead is not an error; from v6 (KIP-1043) a group not found, or one
    /// that is not a classic group, is Dead with GROUP_ID_NOT_FOUND (69) and Kafka's words for which
    /// (`GroupMetadataManager.describeGroups`).
    ///
    /// A group the caller may not DESCRIBE is an entry of its own with 30 and nothing else, and those come first, in the
    /// order asked; the rest follow, each with the operations the caller may do to it if the request asks (v3 and later).
    pub(crate) async fn describe_groups(
        &self,
        version: i16,
        caller: &Caller,
        req: &DescribeGroupsRequest,
    ) -> Outcome {
        let guard = self.guard(caller);
        let mut resp = DescribeGroupsResponse::default();
        let mut asked: Vec<String> = Vec::new();
        for g in &req.groups {
            if guard.group(Operation::Describe, g.0.as_str()) {
                asked.push(g.0.as_str().to_owned());
            } else {
                let mut refused = DescribedGroup::default();
                refused.group_id = g.clone();
                refused.error_code = kafka_wire::ResponseError::GroupAuthorizationFailed.code();
                resp.groups.push(refused);
            }
        }
        let operations = version >= 3 && req.include_authorized_operations;
        for name in kafka_order(&asked) {
            let mut described = self.describe_group(&name).await;
            if version < 6 {
                described.error_code = 0;
                described.error_message = None;
            }
            if operations && described.error_code == 0 {
                described.authorized_operations = guard.group_operations(&name) & self.group_operations_known();
            }
            resp.groups.push(described);
        }
        Outcome::Respond(kafka_wire::api::Response::DescribeGroups(resp))
    }

    /// The operations a group has at the release the broker stands for: DESCRIBE_CONFIGS and ALTER_CONFIGS came with group
    /// configs in Kafka 4 (KIP-848); 2.8 and 3.9 give READ, DELETE and DESCRIBE alone (328, recorded,
    /// `tools/old-profiles-oracle`, `groups`).
    fn group_operations_known(&self) -> i32 {
        if self.config.kafka_major.get() >= 4 {
            !0
        } else {
            !((1 << Operation::DescribeConfigs.code()) | (1 << Operation::AlterConfigs.code()))
        }
    }

    async fn describe_group(&self, name: &str) -> DescribedGroup {
        let mut out = DescribedGroup::default();
        out.group_id = kafka_wire::messages::GroupId(StrBytes::from_string(name.to_owned()));
        let group = GroupId::parse(name).ok();
        let described = match &group {
            Some(g) => self.groups.describe(g).await,
            None => None,
        };
        let Some(d) = described else {
            let has_offsets = group.as_ref().is_some_and(|g| {
                blocking::run(|| self.meta.group_offsets(g)).is_ok_and(|o| !o.is_empty())
            });
            // Known by its offsets or by a record of having had members: Empty, not Dead.
            let life = group
                .as_ref()
                .and_then(|g| blocking::run(|| self.meta.group_life(g)).ok().flatten());
            // An empty consumer group of the new protocol, a share group or a streams group is not described by this API.
            if life
                .as_ref()
                .is_some_and(|l| l.consumer.is_some() || l.share.is_some() || l.streams.is_some())
            {
                return not_found(out, name, "is not a classic group");
            }
            let known = has_offsets || life.is_some();
            if !known {
                return not_found(out, name, "not found");
            }
            out.group_state = StrBytes::from_static_str("Empty");
            if let Some(life) = life {
                out.protocol_type = StrBytes::from_string(life.protocol_type);
            }
            return out;
        };
        // A consumer group is not described by the classic API: it is Dead here, as on Kafka, and ConsumerGroupDescribe
        // has it.
        if d.consumer.is_some() || d.share.is_some() || d.streams.is_some() {
            return not_found(out, name, "is not a classic group");
        }
        fill(&mut out, d);
        out
    }
}

/// A group this API does not describe: Dead, and from v6 GROUP_ID_NOT_FOUND with `Group <id> <why>.` (the caller clears
/// both below v6).
fn not_found(mut out: DescribedGroup, name: &str, why: &str) -> DescribedGroup {
    out.group_state = StrBytes::from_static_str("Dead");
    out.error_code = kafka_wire::ResponseError::GroupIdNotFound.code();
    out.error_message = Some(StrBytes::from_string(format!("Group {name} {why}.")));
    out
}

fn fill(out: &mut DescribedGroup, d: Description) {
    out.group_state = StrBytes::from_static_str(state_name(d.state));
    out.protocol_type = StrBytes::from_string(d.protocol_type.unwrap_or_default());
    // Only a Stable group shows its protocol and its members' metadata: in a rebalance, CompletingRebalance
    // included, Kafka leaves both out and shows the assignments as they are.
    let stable = d.state == State::Stable;
    if stable {
        out.protocol_data = StrBytes::from_string(d.protocol.unwrap_or_default());
    }
    out.members = d
        .members
        .into_iter()
        .map(|m| {
            let mut member = DescribedGroupMember::default();
            member.member_id = StrBytes::from_string(m.id.to_string());
            member.group_instance_id = m.instance.map(|i| StrBytes::from_string(i.to_string()));
            member.client_id = StrBytes::from_string(m.client_id);
            member.client_host = StrBytes::from_string(m.client_host.clone());
            if stable {
                member.member_metadata = m.metadata;
            }
            member.member_assignment = m.assignment;
            member
        })
        .collect();
}

/// The words Kafka uses for a group's state.
pub(super) fn state_name(state: State) -> &'static str {
    match state {
        State::Empty => "Empty",
        State::PreparingRebalance => "PreparingRebalance",
        State::CompletingRebalance => "CompletingRebalance",
        State::Stable => "Stable",
    }
}
