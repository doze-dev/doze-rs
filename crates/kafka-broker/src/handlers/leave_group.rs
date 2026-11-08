//! LeaveGroup.

use kafka_storage::fs::Fs;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use kafka_wire::messages::leave_group_response::MemberResponse;
use kafka_wire::messages::{LeaveGroupRequest, LeaveGroupResponse};
use kafka_wire::{ResponseError, StrBytes};

use crate::groups::{Answer, LeaveTarget, Req};
use crate::{Broker, Outcome};

impl<F: Fs> Broker<F> {
    pub(crate) async fn leave_group(&self, version: i16, req: &LeaveGroupRequest) -> Outcome {
        let mut resp = LeaveGroupResponse::default();
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
                        .and_then(|i| InstanceId::new(i.as_str())),
                })
                .collect()
        } else {
            vec![LeaveTarget {
                member_id: MemberId::new(req.member_id.as_str()),
                instance: None,
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
