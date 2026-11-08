//! JoinGroup.

use kafka_storage::fs::Fs;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use kafka_wire::messages::join_group_response::JoinGroupResponseMember;
use kafka_wire::messages::{JoinGroupRequest, JoinGroupResponse};
use kafka_wire::{ResponseError, StrBytes};

use crate::groups::{Answer, JoinAnswer, JoinRequest, Protocol, Req};
use crate::{Broker, Outcome};

impl<F: Fs> Broker<F> {
    pub(crate) async fn join_group(
        &self,
        version: i16,
        client_id: &str,
        req: &JoinGroupRequest,
    ) -> Outcome {
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
            client_id: client_id.to_owned(),
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
            // Version 0 has no rebalance timeout: the session's stands in for it.
            rebalance_ms: if version >= 1 {
                req.rebalance_timeout_ms
            } else {
                req.session_timeout_ms
            },
            // From version 4 a client with no member id is given one and comes back with it.
            id_first: version >= 4,
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
