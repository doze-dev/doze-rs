//! LeaveGroup.

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use kafka_wire::messages::leave_group_response::MemberResponse;
use kafka_wire::messages::{LeaveGroupRequest, LeaveGroupResponse};
use kafka_wire::{ResponseError, StrBytes};

use crate::groups::{Answer, LeaveTarget, Req};
use crate::{Broker, Caller, Outcome};

impl<F: Fs> Broker<F> {
    pub(crate) async fn leave_group(
        &self,
        version: i16,
        caller: &Caller,
        req: &LeaveGroupRequest,
    ) -> Outcome {
        let mut resp = LeaveGroupResponse::default();
        // The caller must be allowed to READ the group; a refusal is at the top, in every version.
        if !self
            .guard(caller)
            .group(operation::READ, req.group_id.as_str())
        {
            resp.error_code = ResponseError::GroupAuthorizationFailed.code();
            return Outcome::Respond(kafka_wire::api::Response::LeaveGroup(resp));
        }
        let Ok(group) = GroupId::parse(req.group_id.as_str()) else {
            resp.error_code = ResponseError::InvalidGroupId.code();
            return Outcome::Respond(kafka_wire::api::Response::LeaveGroup(resp));
        };
        // Version 3 and later name several members; before that one member leaves, named at the top.
        let targets: Vec<LeaveTarget> = if version >= 3 {
            req.members
                .iter()
                .map(|m| LeaveTarget {
                    member_id: MemberId::new(m.member_id.as_str()),
                    instance: m
                        .group_instance_id
                        .as_ref()
                        .map(|i| InstanceId::new(i.as_str())),
                    // What the client says (v5 and later), clipped as it is read and never logged.
                    reason: m
                        .reason
                        .as_ref()
                        .and_then(|r| crate::groups::Reason::clip(r.as_str())),
                })
                .collect()
        } else {
            vec![LeaveTarget {
                member_id: MemberId::new(req.member_id.as_str()),
                instance: None,
                reason: None,
            }]
        };
        let Some(Answer::Leave(results)) = self.groups.ask(&group, Req::Leave(targets)).await
        else {
            return Outcome::Close;
        };
        if version < 3 {
            resp.error_code = results
                .first()
                .and_then(|r| r.error)
                .map_or(0, |e| e.code());
        } else {
            resp.members = results
                .iter()
                .map(|r| {
                    let mut m = MemberResponse::default();
                    m.member_id = StrBytes::from_string(
                        r.member_id
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                    );
                    m.group_instance_id = r
                        .instance
                        .as_ref()
                        .map(|i| StrBytes::from_string(i.to_string()));
                    m.error_code = r.error.map_or(0, |e| e.code());
                    m
                })
                .collect();
        }
        Outcome::Respond(kafka_wire::api::Response::LeaveGroup(resp))
    }
}
