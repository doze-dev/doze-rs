//! Heartbeat.

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use kafka_wire::messages::{HeartbeatRequest, HeartbeatResponse};

use crate::groups::{Answer, HeartbeatRequest as Beat, Req};
use crate::{Broker, Caller, Outcome};

impl<F: Fs> Broker<F> {
    pub(crate) async fn heartbeat(&self, caller: &Caller, req: &HeartbeatRequest) -> Outcome {
        // The caller must be allowed to READ the group, once, here and not in the group.
        let allowed = self
            .guard(caller)
            .group(operation::READ, req.group_id.as_str());
        let error = match (
            GroupId::parse(req.group_id.as_str()),
            MemberId::new(req.member_id.as_str()),
        ) {
            _ if !allowed => Some(ResponseError::GroupAuthorizationFailed),
            (Err(_), _) => Some(ResponseError::InvalidGroupId),
            (Ok(_), None) => Some(ResponseError::UnknownMemberId),
            (Ok(group), Some(member_id)) => {
                let beat = Beat {
                    member_id,
                    instance: req
                        .group_instance_id
                        .as_ref()
                        .map(|i| InstanceId::new(i.as_str())),
                    generation: req.generation_id,
                    rights: self.guard(caller).topic_rights(),
                };
                match self.groups.ask(&group, Req::Heartbeat(beat)).await {
                    Some(Answer::Heartbeat(error)) => error,
                    _ => return Outcome::Close,
                }
            }
        };
        let mut resp = HeartbeatResponse::default();
        resp.error_code = error.map_or(0, |e| e.code());
        Outcome::Respond(kafka_wire::api::Response::Heartbeat(resp))
    }
}
