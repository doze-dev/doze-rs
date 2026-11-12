//! Requests for the group APIs.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::{Rig, at, caller, head};
use bytes::Bytes;
use kafka_broker::Outcome;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::join_group_request::JoinGroupRequestProtocol;
use kafka_wire::messages::leave_group_request::MemberIdentity;
use kafka_wire::messages::sync_group_request::SyncGroupRequestAssignment;
use kafka_wire::messages::{
    FindCoordinatorRequest, FindCoordinatorResponse, GroupId, HeartbeatRequest, HeartbeatResponse,
    JoinGroupRequest, JoinGroupResponse, LeaveGroupRequest, LeaveGroupResponse, SyncGroupRequest,
    SyncGroupResponse,
};

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A JoinGroup for a consumer: `protocols` are (name, metadata) most preferred first.
pub fn join_request(
    group: &str,
    member: &str,
    instance: Option<&str>,
    protocols: &[(&str, &str)],
) -> JoinGroupRequest {
    let mut req = JoinGroupRequest::default();
    req.group_id = GroupId(text(group));
    req.member_id = text(member);
    req.group_instance_id = instance.map(text);
    req.session_timeout_ms = 10_000;
    req.rebalance_timeout_ms = 30_000;
    req.protocol_type = text("consumer");
    req.protocols = protocols
        .iter()
        .map(|(n, m)| {
            let mut p = JoinGroupRequestProtocol::default();
            p.name = text(n);
            p.metadata = Bytes::from((*m).to_owned());
            p
        })
        .collect();
    req
}

impl Rig {
    pub async fn join_outcome(&self, version: i16, req: JoinGroupRequest) -> Outcome {
        let request = Request::JoinGroup(req);
        self.broker
            .handle(&at(), &caller(), &head(request.api_key(), version), request)
            .await
    }

    pub async fn join(&self, version: i16, req: JoinGroupRequest) -> JoinGroupResponse {
        match self.join_outcome(version, req).await {
            Outcome::Respond(response @ Response::JoinGroup(_)) => {
                kafka_wire::response::encode(1, version, &response)
                    .unwrap_or_else(|e| panic!("JoinGroup v{version} cannot be encoded: {e}"));
                let Response::JoinGroup(r) = response else {
                    unreachable!("matched above")
                };
                r
            }
            other => panic!("no join response: {other:?}"),
        }
    }

    pub async fn sync(
        &self,
        version: i16,
        group: &str,
        member: &str,
        generation: i32,
        assignments: &[(&str, &str)],
    ) -> SyncGroupResponse {
        let mut req = SyncGroupRequest::default();
        req.group_id = GroupId(text(group));
        req.member_id = text(member);
        req.generation_id = generation;
        req.assignments = assignments
            .iter()
            .map(|(m, a)| {
                let mut x = SyncGroupRequestAssignment::default();
                x.member_id = text(m);
                x.assignment = Bytes::from((*a).to_owned());
                x
            })
            .collect();
        match self.ask(version, Request::SyncGroup(req)).await {
            Response::SyncGroup(r) => r,
            other => panic!("not SyncGroup: {other:?}"),
        }
    }

    pub async fn heartbeat(&self, group: &str, member: &str, generation: i32) -> HeartbeatResponse {
        let mut req = HeartbeatRequest::default();
        req.group_id = GroupId(text(group));
        req.member_id = text(member);
        req.generation_id = generation;
        match self.ask(4, Request::Heartbeat(req)).await {
            Response::Heartbeat(r) => r,
            other => panic!("not Heartbeat: {other:?}"),
        }
    }

    pub async fn leave(&self, version: i16, group: &str, members: &[&str]) -> LeaveGroupResponse {
        let mut req = LeaveGroupRequest::default();
        req.group_id = GroupId(text(group));
        if version < 3 {
            req.member_id = text(members[0]);
        } else {
            req.members = members
                .iter()
                .map(|m| {
                    let mut i = MemberIdentity::default();
                    i.member_id = text(m);
                    i
                })
                .collect();
        }
        match self.ask(version, Request::LeaveGroup(req)).await {
            Response::LeaveGroup(r) => r,
            other => panic!("not LeaveGroup: {other:?}"),
        }
    }

    pub async fn find_coordinator(
        &self,
        version: i16,
        keys: &[&str],
        key_type: i8,
    ) -> FindCoordinatorResponse {
        let mut req = FindCoordinatorRequest::default();
        req.key_type = key_type;
        if version >= 4 {
            req.coordinator_keys = keys.iter().map(|k| text(k)).collect();
        } else {
            req.key = text(keys[0]);
        }
        match self.ask(version, Request::FindCoordinator(req)).await {
            Response::FindCoordinator(r) => r,
            other => panic!("not FindCoordinator: {other:?}"),
        }
    }
}
