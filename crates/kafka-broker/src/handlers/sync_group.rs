//! SyncGroup.

use kafka_storage::fs::Fs;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use kafka_wire::messages::{SyncGroupRequest, SyncGroupResponse};
use kafka_wire::{ResponseError, StrBytes};

use crate::groups::{Answer, Req, SyncRequest};
use crate::{Broker, Outcome};

impl<F: Fs> Broker<F> {
    pub(crate) async fn sync_group(&self, req: &SyncGroupRequest) -> Outcome {
        let Ok(group) = GroupId::parse(req.group_id.as_str()) else {
            return respond(Some(ResponseError::InvalidGroupId), None);
        };
        let Some(member_id) = MemberId::new(req.member_id.as_str()) else {
            return respond(Some(ResponseError::UnknownMemberId), None);
        };
        let sync = SyncRequest {
            member_id,
            instance: req
                .group_instance_id
                .as_ref()
                .map(|i| InstanceId::new(i.as_str())),
            generation: req.generation_id,
            protocol_type: req.protocol_type.as_ref().map(|t| t.as_str().to_owned()),
            protocol: req.protocol_name.as_ref().map(|t| t.as_str().to_owned()),
            assignments: req
                .assignments
                .iter()
                .filter_map(|a| Some((MemberId::new(a.member_id.as_str())?, a.assignment.clone())))
                .collect(),
        };
        match self.groups.ask(&group, Req::Sync(sync)).await {
            Some(Answer::Sync(answer)) => {
                // The answer names the protocol the client named, on a success (version 5 and later).
                let named = answer.error.is_none().then_some(req);
                let mut resp = SyncGroupResponse::default();
                resp.error_code = answer.error.map_or(0, |e| e.code());
                resp.assignment = answer.assignment;
                resp.protocol_type = named.and_then(|r| r.protocol_type.clone());
                resp.protocol_name = named.and_then(|r| r.protocol_name.clone());
                Outcome::Respond(kafka_wire::api::Response::SyncGroup(resp))
            }
            _ => Outcome::Close,
        }
    }
}

fn respond(error: Option<ResponseError>, protocol: Option<StrBytes>) -> Outcome {
    let mut resp = SyncGroupResponse::default();
    resp.error_code = error.map_or(0, |e| e.code());
    resp.protocol_name = protocol;
    Outcome::Respond(kafka_wire::api::Response::SyncGroup(resp))
}
