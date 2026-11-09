//! DescribeGroups.

use kafka_storage::fs::Fs;
use kafka_wire::StrBytes;
use kafka_wire::ids::GroupId;
use kafka_wire::messages::describe_groups_response::{DescribedGroup, DescribedGroupMember};
use kafka_wire::messages::{DescribeGroupsRequest, DescribeGroupsResponse};

use crate::groups::{Description, State, kafka_order};
use crate::{Broker, Outcome, blocking};

/// What a client is shown as the member's address. The broker does not yet carry the peer address down to the group
/// APIs, and a development broker is nearly always reached from the same machine.
pub(super) const CLIENT_HOST: &str = "/127.0.0.1";

impl<F: Fs> Broker<F> {
    /// Describes each group named, in the order Kafka answers. A group the broker has never heard of is Dead (not an
    /// error), and one known only by its committed offsets is Empty.
    pub(crate) async fn describe_groups(&self, req: &DescribeGroupsRequest) -> Outcome {
        let mut resp = DescribeGroupsResponse::default();
        let asked: Vec<String> = req.groups.iter().map(|g| g.0.as_str().to_owned()).collect();
        for name in kafka_order(&asked) {
            resp.groups.push(self.describe_group(&name).await);
        }
        Outcome::Respond(kafka_wire::api::Response::DescribeGroups(resp))
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
            // An empty consumer group of the new protocol is not described by this API.
            if life.as_ref().is_some_and(|l| l.consumer.is_some()) {
                out.group_state = StrBytes::from_static_str("Dead");
                return out;
            }
            let known = has_offsets || life.is_some();
            out.group_state = StrBytes::from_static_str(if known { "Empty" } else { "Dead" });
            if let Some(life) = life {
                out.protocol_type = StrBytes::from_string(life.protocol_type);
            }
            return out;
        };
        // A consumer group is not described by the classic API: it is Dead here, as on Kafka, and ConsumerGroupDescribe
        // has it.
        if d.consumer.is_some() {
            out.group_state = StrBytes::from_static_str("Dead");
            return out;
        }
        fill(&mut out, d);
        out
    }
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
            member.client_host = StrBytes::from_static_str(CLIENT_HOST);
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
