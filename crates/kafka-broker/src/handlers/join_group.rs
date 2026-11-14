//! JoinGroup.

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use kafka_wire::messages::join_group_response::JoinGroupResponseMember;
use kafka_wire::messages::{JoinGroupRequest, JoinGroupResponse};
use kafka_wire::{ResponseError, StrBytes};

use crate::groups::{Answer, JoinAnswer, JoinRequest, Protocol, Req};
use crate::handlers::Client;
use crate::{Broker, Caller, Outcome};

impl<F: Fs> Broker<F> {
    /// Joins a member to a group. The caller must be allowed to READ the group, before the group id is judged.
    pub(crate) async fn join_group(
        &self,
        version: i16,
        caller: &Caller,
        client: &Client<'_>,
        req: &JoinGroupRequest,
    ) -> Outcome {
        if !self
            .guard(caller)
            .group(operation::READ, req.group_id.as_str())
        {
            return respond(
                version,
                &JoinAnswer::refused(ResponseError::GroupAuthorizationFailed, None),
            );
        }
        let Ok(group) = GroupId::parse(req.group_id.as_str()) else {
            return respond(
                version,
                &JoinAnswer::refused(ResponseError::InvalidGroupId, None),
            );
        };
        let join = JoinRequest {
            member_id: MemberId::new(req.member_id.as_str()),
            instance: req
                .group_instance_id
                .as_ref()
                .map(|i| InstanceId::new(i.as_str())),
            client_id: client.id.to_owned(),
            client_host: client.host.to_owned(),
            protocol_type: req.protocol_type.as_str().to_owned(),
            protocols: req
                .protocols
                .iter()
                .map(|p| Protocol {
                    name: p.name.as_str().to_owned(),
                    metadata: p.metadata.clone(),
                })
                .collect(),
            session_ms: req.session_timeout_ms,
            rebalance_ms: rebalance_timeout(version, req),
            // From version 4 a client with no member id is given one and comes back with it.
            issued: false,
            id_first: version >= 4,
            rights: self.guard(caller).topic_rights(),
            // What the client says, clipped as it is read and never logged: only the console's story keeps it.
            reason: req
                .reason
                .as_ref()
                .and_then(|r| crate::groups::Reason::clip(r.as_str())),
        };
        match self.groups.ask(&group, Req::Join(join)).await {
            Some(Answer::Join(answer)) => respond(version, &answer),
            // Replaced by a later JoinGroup of the same member. Kafka drops the earlier request and never answers it,
            // and the connection stays as it was; the client gives up on it in its own time. Held here for as long
            // as the member's session would have run (so that a connection nobody closes does not hold on for ever).
            _ => {
                let held = u64::try_from(req.session_timeout_ms).unwrap_or(0);
                tokio::time::sleep(std::time::Duration::from_millis(held)).await;
                Outcome::Close
            }
        }
    }
}

/// Version 0 has no rebalance timeout: the session's stands in for it.
fn rebalance_timeout(version: i16, req: &JoinGroupRequest) -> i32 {
    if version >= 1 {
        req.rebalance_timeout_ms
    } else {
        req.session_timeout_ms
    }
}

fn respond(version: i16, a: &JoinAnswer) -> Outcome {
    let mut resp = JoinGroupResponse::default();
    resp.error_code = a.error.map_or(0, |e| e.code());
    resp.generation_id = a.generation;
    // Before version 7 these are plain strings; from 7 they are null on a refusal.
    let text = |t: &Option<String>| match (t, version >= 7) {
        (Some(t), _) => Some(StrBytes::from_string(t.clone())),
        (None, false) => Some(StrBytes::from_static_str("")),
        (None, true) => None,
    };
    resp.protocol_type = text(&a.protocol_type);
    resp.protocol_name = text(&a.protocol);
    resp.leader = StrBytes::from_string(
        a.leader
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
    );
    resp.member_id = StrBytes::from_string(
        a.member_id
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
    );
    resp.members = a
        .members
        .iter()
        .map(|m| {
            let mut member = JoinGroupResponseMember::default();
            member.member_id = StrBytes::from_string(m.id.to_string());
            member.group_instance_id = m
                .instance
                .as_ref()
                .map(|i| StrBytes::from_string(i.to_string()));
            member.metadata = m.metadata.clone();
            member
        })
        .collect();
    Outcome::Respond(kafka_wire::api::Response::JoinGroup(resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rebalance_timeout_is_the_requests_from_version_one_and_the_sessions_before() {
        let mut req = JoinGroupRequest::default();
        req.session_timeout_ms = 10_000;
        req.rebalance_timeout_ms = 60_000;
        assert_eq!(rebalance_timeout(0, &req), 10_000);
        assert_eq!(rebalance_timeout(1, &req), 60_000);
        assert_eq!(rebalance_timeout(9, &req), 60_000);
    }
}
